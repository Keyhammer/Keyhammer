// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! # keyhammer-c
//!
//! A stable C ABI over the `keyhammer` core: a small handle-based API with no
//! callbacks, UTF-8 strings only and plain result structs, meant as the base
//! for wrappers in other languages. The header `include/keyhammer.h` is
//! generated from this file by cbindgen; the ABI rules are in
//! `docs/design/c-abi.md`.
//!
//! # Overview
//!
//! 1. `kh_index_build` builds an index from an array of `kh_entry`
//!    (`term` pointer, byte length, weight) and returns an opaque
//!    `kh_index` handle.
//! 2. `kh_search` searches it and fills a `kh_results` with an array of
//!    `kh_hit`. Release it with `kh_results_free`.
//! 3. `kh_index_free` releases the index.
//!
//! Every function that can fail returns a `kh_status`; on failure
//! `kh_last_error` returns a human-readable message for the calling thread.
//!
//! # Text handling
//!
//! Terms and queries are UTF-8 byte ranges given as `(pointer, length)`,
//! never NUL-terminated. Invalid UTF-8 is rejected. Terms and queries are
//! normalised identically before they are compared, with the core's
//! `keyhammer::text::Normalizer`: by default case and diacritics are folded
//! (`São Paulo` matches `sao paulo` and `SAO PAULO`, `Straße` matches
//! `strasse`), and `kh_index_build_ex` can turn either folding off. A hit's
//! `term` is the dictionary text as the caller gave it (original case and
//! accents), and `input_index` is its position in the entry array, so a
//! caller can map a hit back to its own record. Terms that are equal after
//! normalisation are merged (highest weight, then the first). The query limit
//! is `KH_MAX_QUERY_LEN` code points, counted after normalisation.
//!
//! # Threads
//!
//! An index is immutable after it is built, so any number of threads may
//! search the same index at once. Freeing an index while another thread uses
//! it is a data race and is the caller's responsibility to prevent. The last
//! error message is per thread.
//!
//! # Panics
//!
//! No panic crosses the boundary: every entry point runs its body inside
//! `catch_unwind` and reports [`kh_status::KH_ERR_INTERNAL`]. That protection
//! needs the default `panic = "unwind"`; if this crate is built with
//! `panic = "abort"` a panic aborts the process instead (documented, not
//! prevented). A caught panic still prints its message through the host's
//! panic hook (stderr by default). Allocation failure also aborts, as everywhere in Rust's
//! standard collections.

#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#![warn(missing_docs)]
// The exported names are the C names.
#![allow(non_camel_case_types)]

use core::ffi::c_char;
use core::ptr;
use std::cell::RefCell;
use std::ffi::CString;
use std::panic::{AssertUnwindSafe, catch_unwind};

use keyhammer::cost::CostModel;
use keyhammer::search::{Ranking, SearchConfig, SearchError, Searcher};
use keyhammer::text::Normalizer;
use keyhammer::trie::{BuildError, Trie};

/// The ABI version. It changes only when an incompatible change is made; see
/// `docs/design/c-abi.md`. Compare it with `kh_abi_version` at run time.
pub const KH_ABI_VERSION: u32 = 2;
/// `kh_config.ranking`: order by cost rounded up to whole edit units.
pub const KH_RANKING_COARSE: u32 = 0;
/// `kh_config.ranking`: order by the exact weighted cost.
pub const KH_RANKING_EXACT: u32 = 1;
/// The longest accepted query, in code points after normalisation (ABI
/// version 1 counted bytes; see `docs/design/c-abi.md`).
pub const KH_MAX_QUERY_LEN: usize = 128;
/// The longest accepted query, in UTF-8 bytes: 4 bytes for each of
/// `KH_MAX_QUERY_LEN` code points. Longer input is refused before it is read.
pub const KH_MAX_QUERY_BYTES: usize = 4 * KH_MAX_QUERY_LEN;
/// `kh_index_build_ex` flag: do not fold case (`A` and `a` differ).
pub const KH_NORM_KEEP_CASE: u32 = 1;
/// `kh_index_build_ex` flag: do not fold diacritics (`é` and `e` differ).
pub const KH_NORM_KEEP_DIACRITICS: u32 = 2;

/// Result of a call. `KH_OK` is 0; every failure is non-zero. New codes may
/// be added in later minor versions, so treat unknown non-zero values as
/// failures.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum kh_status {
    /// Success.
    KH_OK = 0,
    /// A required pointer argument was null.
    KH_ERR_NULL_POINTER = 1,
    /// A length does not fit (larger than `isize::MAX`, or more entries than
    /// an index can hold).
    KH_ERR_INVALID_LENGTH = 2,
    /// A term or query is not valid UTF-8.
    KH_ERR_INVALID_UTF8 = 3,
    /// An argument has an invalid value (unknown ranking, budget too large,
    /// `struct_size` too small).
    KH_ERR_INVALID_ARGUMENT = 4,
    /// The entry array is empty.
    KH_ERR_EMPTY_INDEX = 5,
    /// A term is empty.
    KH_ERR_EMPTY_TERM = 6,
    /// A term is longer than 65535 bytes.
    KH_ERR_TERM_TOO_LONG = 7,
    /// The query is longer than `KH_MAX_QUERY_LEN` code points after
    /// normalisation, or than `KH_MAX_QUERY_BYTES` bytes.
    KH_ERR_QUERY_TOO_LONG = 8,
    /// An internal error (a caught panic). The message is in `kh_last_error`.
    KH_ERR_INTERNAL = 9,
}

/// One dictionary entry given to `kh_index_build`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct kh_entry {
    /// The term: `len` bytes of UTF-8, not NUL-terminated.
    pub term: *const u8,
    /// Length of the term in bytes (1 to 65535).
    pub len: usize,
    /// The term's weight (higher ranks first among equal costs).
    pub weight: u16,
}

/// Search parameters. Call `kh_config_default` to fill it, then change
/// fields. `struct_size` must be `sizeof(kh_config)` as seen by the caller;
/// it lets a library newer than the header still read this struct.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct kh_config {
    /// `sizeof(kh_config)` of the header the caller was compiled with.
    pub struct_size: u32,
    /// Number of results wanted (default 10; 0 returns no hits).
    pub k: u32,
    /// Largest accepted edit cost, fixed point with 16 = one ordinary edit
    /// (default 32; at most 64, else `KH_ERR_INVALID_ARGUMENT`).
    pub budget: u32,
    /// `KH_RANKING_COARSE` (default) or `KH_RANKING_EXACT`.
    pub ranking: u32,
    /// Hard limit on expanded trie nodes (default 100000); the search stops
    /// early and reports `truncated` when it is reached.
    pub max_nodes: u64,
    /// Non-zero enables the subtree-signature lower bound (does not change
    /// the results). Default 0.
    pub tsb: u32,
    /// Must be 0. It makes `sizeof(kh_config)` identical on all targets (no
    /// implicit tail padding), so that appended fields grow the struct.
    pub reserved: u32,
}

/// One search hit. `term` points into the index and stays valid until the
/// index is freed; it is not NUL-terminated.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct kh_hit {
    /// The matched term as it was given to `kh_index_build`, `term_len` bytes
    /// of UTF-8 (original case and accents, not the normalised form).
    /// For terms merged by normalisation, the entry that was kept.
    pub term: *const u8,
    /// Length of `term` in bytes.
    pub term_len: usize,
    /// Term id inside the index (position in the byte-sorted, deduplicated
    /// list of normalised terms).
    pub id: u32,
    /// Exact weighted edit cost (16 = one ordinary edit), whatever the ranking.
    pub cost: u32,
    /// The term's weight.
    pub weight: u32,
    /// Position in the `kh_entry` array given to `kh_index_build` of the
    /// entry this hit is (the one kept among terms equal after
    /// normalisation).
    pub input_index: u32,
}

/// The output of `kh_search`. Free it with `kh_results_free`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct kh_results {
    /// `len` hits, best first; null when `len` is 0.
    pub hits: *mut kh_hit,
    /// Number of hits.
    pub len: usize,
    /// Trie nodes expanded by the search.
    pub nodes_expanded: u64,
    /// Non-zero if `max_nodes` stopped the search early.
    pub truncated: u32,
}

/// An opaque index handle. Create it with `kh_index_build`, free it with
/// `kh_index_free`.
pub struct kh_index {
    trie: Trie,
    /// The kept entries' terms as given, by term id.
    originals: Vec<Box<str>>,
    costs: CostModel,
}

// The handle may be shared by threads that only search it.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<kh_index>();
};

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn set_error(msg: impl Into<Vec<u8>>) {
    // Interior NULs cannot occur in our own messages; if one did, keep an
    // empty message instead of failing.
    let c = CString::new(msg).unwrap_or_default();
    let _ = LAST_ERROR.try_with(|e| {
        if let Ok(mut e) = e.try_borrow_mut() {
            *e = c;
        }
    });
}

fn fail(status: kh_status, msg: impl Into<Vec<u8>>) -> kh_status {
    set_error(msg);
    status
}

/// Runs `f`, turning a panic into `KH_ERR_INTERNAL`.
fn guard(f: impl FnOnce() -> kh_status) -> kh_status {
    set_error("");
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| {
        fail(
            kh_status::KH_ERR_INTERNAL,
            "internal error: a panic was caught at the FFI boundary",
        )
    })
}

/// Returns `KH_ABI_VERSION` of the loaded library. A wrapper should check
/// that it equals the version of the header it was built against.
#[unsafe(no_mangle)]
pub extern "C" fn kh_abi_version() -> u32 {
    KH_ABI_VERSION
}

/// The message of the last failed call on the calling thread, as a
/// NUL-terminated UTF-8 string; empty if the last call succeeded. The pointer
/// is valid until the next call into this library on the same thread. Never
/// null.
#[unsafe(no_mangle)]
pub extern "C" fn kh_last_error() -> *const c_char {
    // The thread-local CString is only replaced by `set_error`, which runs at
    // the start of the next call, so the pointer is stable until then.
    LAST_ERROR
        .try_with(|e| e.try_borrow().map_or(c"".as_ptr(), |e| e.as_ptr()))
        .unwrap_or(c"".as_ptr())
}

/// A static, NUL-terminated description of a status code. Any integer is
/// accepted (it is not an enum on purpose: a value outside the enum is
/// undefined behaviour in Rust); unknown codes give "unknown status". Never
/// null.
#[unsafe(no_mangle)]
pub extern "C" fn kh_status_string(status: i32) -> *const c_char {
    match status {
        0 => c"ok",
        1 => c"null pointer",
        2 => c"invalid length",
        3 => c"invalid UTF-8",
        4 => c"invalid argument",
        5 => c"no entries",
        6 => c"empty term",
        7 => c"term too long",
        8 => c"query too long",
        9 => c"internal error",
        _ => c"unknown status",
    }
    .as_ptr()
}

impl kh_config {
    fn defaults() -> Self {
        let d = SearchConfig::default();
        Self {
            struct_size: size_of::<kh_config>() as u32,
            k: u32::try_from(d.k).unwrap_or(u32::MAX),
            budget: u32::from(d.budget),
            ranking: KH_RANKING_COARSE,
            max_nodes: d.max_nodes as u64,
            tsb: 0,
            reserved: 0,
        }
    }
}

/// Fills `cfg` with the defaults (`k = 10`, `budget = 32`, coarse ranking,
/// `max_nodes = 100000`, `tsb = 0`) and sets `struct_size`.
///
/// # Safety
///
/// `cfg` must be null or valid for writing a `kh_config`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kh_config_default(cfg: *mut kh_config) -> kh_status {
    guard(|| {
        if cfg.is_null() {
            return fail(kh_status::KH_ERR_NULL_POINTER, "cfg is null");
        }
        // SAFETY: `cfg` is non-null and, by the caller's contract, valid for
        // writing one `kh_config`.
        unsafe { cfg.write(kh_config::defaults()) };
        kh_status::KH_OK
    })
}

/// Turns a `(ptr, len)` from C into a byte slice.
///
/// # Safety
///
/// If `ptr` is not null and `len` is not 0, `ptr` must point to `len`
/// initialised bytes that stay valid and unmodified for `'a`.
unsafe fn bytes<'a>(ptr: *const u8, len: usize, what: &str) -> Result<&'a [u8], kh_status> {
    if len == 0 {
        return Ok(&[]);
    }
    if ptr.is_null() {
        set_error(format!("{what}: null pointer with length {len}"));
        return Err(kh_status::KH_ERR_NULL_POINTER);
    }
    if len > isize::MAX as usize {
        set_error(format!("{what}: length {len} exceeds isize::MAX"));
        return Err(kh_status::KH_ERR_INVALID_LENGTH);
    }
    // SAFETY: `ptr` is non-null, `len` fits in `isize`, and by the caller's
    // contract `ptr` points to `len` initialised bytes valid for `'a`.
    Ok(unsafe { core::slice::from_raw_parts(ptr, len) })
}

/// Builds an index from `n` entries and stores the handle in `*out`.
///
/// Terms are normalised (case and diacritics folded, as `kh_index_build_ex`
/// with no flags); terms equal after normalisation are merged, keeping the
/// highest weight (then the first). Hits return the term as given. On success `*out` is a new handle that must be released with
/// `kh_index_free`; on failure `*out` is set to null. The entries and their
/// term bytes are copied and need not outlive the call. Embedded NUL bytes in
/// a term are accepted and returned unchanged (terms are not C strings).
///
/// Fails with `KH_ERR_NULL_POINTER` (`out` or `entries` null, or a term
/// pointer null with a non-zero length), `KH_ERR_INVALID_LENGTH`,
/// `KH_ERR_EMPTY_INDEX` (`n == 0`), `KH_ERR_EMPTY_TERM`,
/// `KH_ERR_TERM_TOO_LONG` or `KH_ERR_INVALID_UTF8`. A term that normalises to
/// nothing (only combining marks) fails with `KH_ERR_EMPTY_TERM`, and one that
/// normalises to more than 65535 bytes with `KH_ERR_TERM_TOO_LONG`.
///
/// # Safety
///
/// `out` must be null or valid for writing a pointer. `entries` must be null
/// (only with `n == 0`) or point to `n` initialised `kh_entry`, each of
/// whose `term` is null (only with `len == 0`) or points to `len` initialised
/// bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kh_index_build(
    entries: *const kh_entry,
    n: usize,
    out: *mut *mut kh_index,
) -> kh_status {
    // SAFETY: forwarded from this function's contract.
    unsafe { kh_index_build_ex(entries, n, 0, out) }
}

/// `kh_index_build` with normalisation flags: 0 folds case and diacritics (the
/// default), `KH_NORM_KEEP_CASE` and `KH_NORM_KEEP_DIACRITICS` (ORed) turn a
/// folding off. Queries are normalised the same way as the terms. Any other
/// bit fails with `KH_ERR_INVALID_ARGUMENT`; the other failures are those of
/// `kh_index_build`. With `KH_NORM_KEEP_DIACRITICS` there is no Unicode
/// composition: `é` (one code point) and `e` followed by U+0301 differ, and
/// the second costs an extra edit.
///
/// # Safety
///
/// As `kh_index_build`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kh_index_build_ex(
    entries: *const kh_entry,
    n: usize,
    flags: u32,
    out: *mut *mut kh_index,
) -> kh_status {
    guard(|| {
        if out.is_null() {
            return fail(kh_status::KH_ERR_NULL_POINTER, "out is null");
        }
        // SAFETY: `out` is non-null and valid for writing a pointer by the
        // caller's contract.
        unsafe { out.write(ptr::null_mut()) };
        if flags & !(KH_NORM_KEEP_CASE | KH_NORM_KEEP_DIACRITICS) != 0 {
            return fail(
                kh_status::KH_ERR_INVALID_ARGUMENT,
                format!("unknown normalisation flags {flags:#x}"),
            );
        }
        let normalizer = Normalizer::new()
            .with_case_folding(flags & KH_NORM_KEEP_CASE == 0)
            .with_diacritic_folding(flags & KH_NORM_KEEP_DIACRITICS == 0);
        if n == 0 {
            return fail(kh_status::KH_ERR_EMPTY_INDEX, "no entries were given");
        }
        if entries.is_null() {
            return fail(kh_status::KH_ERR_NULL_POINTER, "entries is null");
        }
        let too_big = n >= u32::MAX as usize
            || n.checked_mul(size_of::<kh_entry>())
                .is_none_or(|b| b > isize::MAX as usize);
        if too_big {
            return fail(
                kh_status::KH_ERR_INVALID_LENGTH,
                format!("{n} entries do not fit in an index"),
            );
        }
        // SAFETY: `entries` is non-null and, by the caller's contract, points
        // to `n` initialised entries; `n * size_of` fits in `isize` (checked).
        let entries = unsafe { core::slice::from_raw_parts(entries, n) };
        let mut terms: Vec<(&str, u16)> = Vec::new();
        if terms.try_reserve_exact(n).is_err() {
            return fail(kh_status::KH_ERR_INTERNAL, "out of memory");
        }
        for (i, e) in entries.iter().enumerate() {
            if e.len == 0 {
                return fail(
                    kh_status::KH_ERR_EMPTY_TERM,
                    format!("entry {i}: empty term"),
                );
            }
            // SAFETY: forwarded from this function's contract for `e.term`.
            let b = match unsafe { bytes(e.term, e.len, &format!("entry {i}")) } {
                Ok(b) => b,
                Err(s) => return s,
            };
            let Ok(s) = core::str::from_utf8(b) else {
                return fail(
                    kh_status::KH_ERR_INVALID_UTF8,
                    format!("entry {i}: term is not valid UTF-8"),
                );
            };
            if s.len() > usize::from(u16::MAX) {
                return fail(
                    kh_status::KH_ERR_TERM_TOO_LONG,
                    format!("entry {i}: term is longer than 65535 bytes"),
                );
            }
            if normalizer.normalize(s).is_empty() {
                return fail(
                    kh_status::KH_ERR_EMPTY_TERM,
                    format!("entry {i}: term normalises to nothing"),
                );
            }
            terms.push((s, e.weight));
        }
        let trie = match Trie::build_normalized(&terms, &normalizer) {
            Ok(t) => t,
            Err(BuildError::EmptyTerm) => {
                return fail(kh_status::KH_ERR_EMPTY_TERM, "empty term");
            }
            Err(BuildError::TermTooLong) => {
                return fail(
                    kh_status::KH_ERR_TERM_TOO_LONG,
                    "a term is longer than 65535 bytes",
                );
            }
            Err(BuildError::TooManyTerms) => {
                return fail(kh_status::KH_ERR_INVALID_LENGTH, "too many terms");
            }
            Err(e) => return fail(kh_status::KH_ERR_INVALID_ARGUMENT, e.to_string()),
        };
        let originals: Vec<Box<str>> = (0..trie.len())
            .map(|id| {
                let i = trie.input_index(u32::try_from(id).unwrap_or(u32::MAX)) as usize;
                terms.get(i).map_or("", |t| t.0).into()
            })
            .collect();
        let index = Box::new(kh_index {
            trie,
            originals,
            costs: CostModel::qwerty(),
        });
        // SAFETY: `out` is valid for writing (see above).
        unsafe { out.write(Box::into_raw(index)) };
        kh_status::KH_OK
    })
}

/// Frees an index and sets `*index` to null, so that freeing the same
/// variable twice is a no-op. A null `index` or `*index` is ignored. Hits
/// (their `term` pointers) obtained from this index must not be used after
/// this call; a `kh_results` itself stays valid and must still be freed.
///
/// # Safety
///
/// `index` must be null or point to a pointer that is null or came from
/// `kh_index_build` and has not been freed through any other copy. No other
/// thread may be using the index.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kh_index_free(index: *mut *mut kh_index) {
    let _ = guard(|| {
        if index.is_null() {
            return kh_status::KH_OK;
        }
        // SAFETY: `index` is non-null and, by the caller's contract, points
        // to a readable and writable pointer.
        let handle = unsafe { index.replace(ptr::null_mut()) };
        if !handle.is_null() {
            // SAFETY: a non-null handle came from `Box::into_raw` in
            // `kh_index_build` and has not been freed (caller's contract).
            drop(unsafe { Box::from_raw(handle) });
        }
        kh_status::KH_OK
    });
}

/// Stores the number of distinct terms of the index in `*out`.
///
/// # Safety
///
/// `index` must be null or a live handle; `out` must be null or valid for
/// writing a `size_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kh_index_len(index: *const kh_index, out: *mut usize) -> kh_status {
    guard(|| {
        if index.is_null() || out.is_null() {
            return fail(kh_status::KH_ERR_NULL_POINTER, "index or out is null");
        }
        // SAFETY: `index` is a live handle (caller's contract) and `out` is
        // valid for writing.
        unsafe { out.write((*index).trie.len()) };
        kh_status::KH_OK
    })
}

const EMPTY_RESULTS: kh_results = kh_results {
    hits: ptr::null_mut(),
    len: 0,
    nodes_expanded: 0,
    truncated: 0,
};

/// Reads a caller's config, applying the `struct_size` rule.
///
/// # Safety
///
/// `cfg` must be null or point to `(*cfg).struct_size` readable bytes.
unsafe fn read_config(cfg: *const kh_config) -> Result<SearchConfig, kh_status> {
    if cfg.is_null() {
        return Ok(SearchConfig::default());
    }
    // SAFETY: `cfg` is non-null and `struct_size` is the first field, at
    // offset 0; the caller's contract makes at least those 4 bytes readable.
    let size = unsafe { cfg.cast::<u32>().read_unaligned() } as usize;
    if size < size_of::<kh_config>() {
        set_error(format!(
            "cfg.struct_size is {size}, expected at least {}",
            size_of::<kh_config>()
        ));
        return Err(kh_status::KH_ERR_INVALID_ARGUMENT);
    }
    // SAFETY: `size >= size_of::<kh_config>()` bytes are readable at `cfg`
    // (caller's contract) and every bit pattern is a valid `kh_config`.
    let c = unsafe { cfg.read_unaligned() };
    if c.reserved != 0 {
        set_error("cfg.reserved must be 0");
        return Err(kh_status::KH_ERR_INVALID_ARGUMENT);
    }
    let ranking = match c.ranking {
        KH_RANKING_COARSE => Ranking::Coarse,
        KH_RANKING_EXACT => Ranking::Exact,
        other => {
            set_error(format!("unknown ranking {other}"));
            return Err(kh_status::KH_ERR_INVALID_ARGUMENT);
        }
    };
    let Ok(budget) = u16::try_from(c.budget) else {
        set_error(format!("budget {} does not fit in 16 bits", c.budget));
        return Err(kh_status::KH_ERR_INVALID_ARGUMENT);
    };
    Ok(SearchConfig {
        k: c.k as usize,
        budget,
        tsb: c.tsb != 0,
        max_nodes: usize::try_from(c.max_nodes).unwrap_or(usize::MAX),
        ranking,
    })
}

/// Searches `index` for the UTF-8 query `(query, query_len)` and fills `*out`.
///
/// `cfg` may be null for the defaults. `*out` is fully overwritten (it is not
/// read, so an uninitialised struct is fine, but a previous result in it is
/// leaked unless freed first); on failure it is set to an empty result. On
/// success release it with `kh_results_free`. The query is normalised like
/// the terms (`ACAO` finds `ação`). An empty query is allowed. Embedded NUL bytes are ordinary
/// bytes. The pointer returned by `kh_last_error` is invalidated by the next
/// call into the library, including `kh_index_free` and `kh_results_free`.
/// Copying a `kh_results` and freeing both copies is a double free, like
/// copying the handle.
///
/// Fails with `KH_ERR_NULL_POINTER` (`index` or `out` null, or `query` null
/// with a non-zero length), `KH_ERR_INVALID_UTF8`,
/// `KH_ERR_QUERY_TOO_LONG` (`query_len` above `KH_MAX_QUERY_BYTES`, checked
/// before the buffer is read, so a huge length is reported this way and
/// `KH_ERR_INVALID_LENGTH` is never returned by this function; or more than
/// `KH_MAX_QUERY_LEN` code points after normalisation) or
/// `KH_ERR_INVALID_ARGUMENT` (bad `cfg`, including `reserved != 0`).
///
/// # Safety
///
/// `index` must be null or a live handle. `query` must be null (only with
/// `query_len == 0`) or point to `query_len` initialised bytes. `cfg` must be
/// null or point to `cfg->struct_size` readable bytes. `out` must be null or
/// valid for writing a `kh_results`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kh_search(
    index: *const kh_index,
    query: *const u8,
    query_len: usize,
    cfg: *const kh_config,
    out: *mut kh_results,
) -> kh_status {
    guard(|| {
        if out.is_null() {
            return fail(kh_status::KH_ERR_NULL_POINTER, "out is null");
        }
        // SAFETY: `out` is non-null and valid for writing.
        unsafe { out.write(EMPTY_RESULTS) };
        if index.is_null() {
            return fail(kh_status::KH_ERR_NULL_POINTER, "index is null");
        }
        // SAFETY: forwarded from this function's contract.
        let cfg = match unsafe { read_config(cfg) } {
            Ok(c) => c,
            Err(s) => return s,
        };
        if query_len > KH_MAX_QUERY_BYTES {
            return fail(
                kh_status::KH_ERR_QUERY_TOO_LONG,
                format!("query has {query_len} bytes, the limit is {KH_MAX_QUERY_BYTES}"),
            );
        }
        // SAFETY: forwarded from this function's contract.
        let q = match unsafe { bytes(query, query_len, "query") } {
            Ok(q) => q,
            Err(s) => return s,
        };
        let Ok(q) = core::str::from_utf8(q) else {
            return fail(kh_status::KH_ERR_INVALID_UTF8, "query is not valid UTF-8");
        };
        // SAFETY: `index` is a live handle (caller's contract); only shared
        // access is used, so concurrent searches are fine.
        let index = unsafe { &*index };
        let mut searcher = Searcher::new();
        let output = match searcher.search_text(&index.trie, &index.costs, q, &cfg) {
            Ok(o) => o,
            Err(SearchError::QueryTooLong { len, max }) => {
                return fail(
                    kh_status::KH_ERR_QUERY_TOO_LONG,
                    format!("query has {len} code points after normalisation, the limit is {max}"),
                );
            }
            Err(e) => return fail(kh_status::KH_ERR_INVALID_ARGUMENT, e.to_string()),
        };
        let hits: Box<[kh_hit]> = output
            .hits
            .iter()
            .map(|h| {
                let i = index.trie.input_index(h.id);
                let t = index
                    .originals
                    .get(h.id as usize)
                    .map_or_else(|| index.trie.term(h.id), |t| &**t);
                kh_hit {
                    term: t.as_ptr(),
                    term_len: t.len(),
                    id: h.id,
                    cost: u32::from(h.cost),
                    weight: u32::from(h.weight),
                    input_index: i,
                }
            })
            .collect();
        let len = hits.len();
        let hits_ptr = if len == 0 {
            ptr::null_mut()
        } else {
            Box::into_raw(hits).cast::<kh_hit>()
        };
        let res = kh_results {
            hits: hits_ptr,
            len,
            nodes_expanded: output.stats.nodes_expanded as u64,
            truncated: u32::from(output.stats.truncated),
        };
        // SAFETY: `out` is valid for writing (see above).
        unsafe { out.write(res) };
        kh_status::KH_OK
    })
}

/// Frees the hits of a `kh_results` and resets it to the empty result, so
/// that freeing it twice is a no-op. Null and empty results are ignored.
///
/// # Safety
///
/// `results` must be null or point to a `kh_results` that was filled by
/// `kh_search` (or zero-initialised) and not modified by the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kh_results_free(results: *mut kh_results) {
    let _ = guard(|| {
        if results.is_null() {
            return kh_status::KH_OK;
        }
        // SAFETY: `results` is non-null and readable/writable (contract).
        let r = unsafe { results.replace(EMPTY_RESULTS) };
        if !r.hits.is_null() && r.len != 0 {
            // SAFETY: `hits`/`len` were produced by `Box::into_raw` of a
            // `Box<[kh_hit]>` of exactly `len` elements in `kh_search`, and
            // the reset above makes a second free a no-op.
            drop(unsafe { Box::from_raw(ptr::slice_from_raw_parts_mut(r.hits, r.len)) });
        }
        kh_status::KH_OK
    });
}

#[cfg(test)]
mod tests;

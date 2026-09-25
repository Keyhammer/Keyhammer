// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! # keyhammer-wasm
//!
//! A WebAssembly build of the `keyhammer` core with a tiny C-ABI, meant to be
//! loaded with the plain `WebAssembly` API (no generated glue code). Status:
//! unpublished prototype; the interface may change at any time.
//!
//! The module holds a single index in module state (WebAssembly is
//! single-threaded here, so a `thread_local!` `RefCell` is enough).
//!
//! # Interface
//!
//! - [`kh_alloc`] / [`kh_free`]: a buffer in linear memory that JavaScript
//!   fills with UTF-8 bytes (dictionary text or a query) and frees afterwards
//!   with the same length.
//! - [`kh_build`]: builds the index from UTF-8 text, one term per line with an
//!   optional `TAB weight` (a `u16`, default 0). Terms are compared after
//!   the core's default normalisation (`keyhammer::text::Normalizer::new`:
//!   case folding and diacritic folding, so `São Paulo` is `sao paulo` and
//!   `Straße` is `strasse`). Lines that are empty, longer than 65535 bytes,
//!   whose weight is not a `u16`, or that normalise to nothing (only
//!   combining marks) or to more than 65535 bytes are skipped. Terms that are equal
//!   after normalisation are merged (highest weight, then first). Returns
//!   the number of distinct terms loaded, or 0 on failure, in which case
//!   there is no index any more.
//! - [`kh_search`]: runs a search; returns the number of hits, or `u32::MAX`
//!   on error (no index, query longer than 128 code points after
//!   normalisation (a `ß` counts as two), budget too large, unknown ranking,
//!   invalid UTF-8). The query is normalised like the terms.
//! - [`kh_results_ptr`] / [`kh_results_len`]: the text of the last search, in
//!   UTF-8. The first line is a header, `nodes_expanded TAB truncated` (the
//!   number of trie nodes expanded and `1` if the node limit stopped the
//!   search early, else `0`). Then one line per hit, best first:
//!   `term TAB cost TAB weight`, where `term` is the dictionary text as it was
//!   given to [`kh_build`] (original case and accents; for merged terms, the
//!   entry that was kept), not the normalised form, and where `cost` is the exact fixed-point cost
//!   (16 = one ordinary edit). Every line ends with `\n`. After an error the
//!   text is empty. The pointer is valid until the next call to
//!   [`kh_build`] or [`kh_search`].
//!
//! # Panics
//!
//! None of these functions panics on any input: lengths are checked before
//! any slice is formed, allocation failures are reported instead of aborting,
//! and the code uses no `unwrap`, `expect` or indexing of its own. (The size
//! profile uses `panic = "abort"`, so a panic would trap the module.) Running
//! out of memory inside the core, which allocates infallibly, still traps.

#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
#![warn(missing_docs)]

use core::cell::RefCell;

use keyhammer::cost::CostModel;
use keyhammer::search::{Ranking, SearchConfig, Searcher};
use keyhammer::text::Normalizer;
use keyhammer::trie::Trie;

/// Returned by [`kh_search`] on error.
const ERROR: u32 = u32::MAX;

struct State {
    trie: Option<Trie>,
    /// The terms as given (trimmed), in the order of the lines kept, so that
    /// `Trie::input_index` maps a hit back to its original text.
    originals: Vec<Box<str>>,
    costs: CostModel,
    searcher: Searcher,
    results: String,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        trie: None,
        originals: Vec::new(),
        costs: CostModel::qwerty(),
        searcher: Searcher::new(),
        results: String::new(),
    });
}

/// Runs `f` on the module state, or returns `None` if the state is not
/// reachable (never expected: there is no re-entrancy and no thread exit).
fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE
        .try_with(|s| s.try_borrow_mut().ok().map(|mut s| f(&mut s)))
        .ok()
        .flatten()
}

/// Turns a JavaScript-provided `(ptr, len)` into a byte slice, or `None` if
/// it cannot describe a valid slice.
///
/// # Safety
///
/// If `ptr` is not null and `len` is not 0, `ptr` must point to `len`
/// initialised bytes that stay valid and unmodified for `'a`.
unsafe fn bytes<'a>(ptr: *const u8, len: u32) -> Option<&'a [u8]> {
    let len = usize::try_from(len).ok()?;
    if len == 0 {
        return Some(&[]);
    }
    if ptr.is_null() || len > isize::MAX as usize {
        return None;
    }
    // SAFETY: `ptr` is non-null and, by the caller's contract, points to
    // `len` initialised bytes valid for `'a`; `len` fits in `isize`.
    Some(unsafe { core::slice::from_raw_parts(ptr, len) })
}

/// Allocates `len` bytes in linear memory for JavaScript to fill. Returns
/// null if the allocation fails. A `len` of 0 returns a non-null dangling
/// pointer that must not be read or written. Free it with [`kh_free`] and
/// the same `len`.
#[unsafe(no_mangle)]
pub extern "C" fn kh_alloc(len: u32) -> *mut u8 {
    let Ok(len) = usize::try_from(len) else {
        return core::ptr::null_mut();
    };
    let mut v: Vec<u8> = Vec::new();
    // `try_reserve_exact` reports capacity overflow and allocation failure
    // instead of panicking.
    if v.try_reserve_exact(len).is_err() {
        return core::ptr::null_mut();
    }
    let mut v = core::mem::ManuallyDrop::new(v);
    v.as_mut_ptr()
}

/// Frees a buffer returned by [`kh_alloc`]. Null pointers and `len == 0` are
/// ignored.
///
/// # Safety
///
/// `ptr` must come from [`kh_alloc`] called with the same `len`, and must not
/// be freed twice or used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kh_free(ptr: *mut u8, len: u32) {
    let Ok(len) = usize::try_from(len) else {
        return;
    };
    if ptr.is_null() || len == 0 {
        return;
    }
    // SAFETY: by the caller's contract `ptr` was returned by `kh_alloc(len)`,
    // i.e. it is the buffer of a `Vec<u8>` whose capacity is exactly `len`
    // (`try_reserve_exact` on an empty vector); a length of 0 is always valid.
    drop(unsafe { Vec::from_raw_parts(ptr, 0, len) });
}

/// Parses one dictionary line into `(term, weight)`, or `None` to skip it.
fn parse_line(line: &str) -> Option<(&str, u16)> {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let (term, weight) = match line.split_once('\t') {
        Some((t, w)) => (t, w.trim().parse::<u16>().ok()?),
        None => (line, 0),
    };
    let term = term.trim();
    if term.is_empty() || term.len() > usize::from(u16::MAX) {
        return None;
    }
    Some((term, weight))
}

/// Builds the index from UTF-8 text, one term per line with an optional
/// `TAB weight`. Returns the number of distinct terms loaded, or 0 on failure (invalid
/// UTF-8, no valid line, out of memory); after a failure there is no index.
///
/// # Safety
///
/// `ptr` must point to `len` initialised bytes (for example a buffer from
/// [`kh_alloc`]), unless `len` is 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kh_build(ptr: *const u8, len: u32) -> u32 {
    // SAFETY: forwarded from this function's contract.
    let input = unsafe { bytes(ptr, len) };
    with_state(|s| {
        s.trie = None;
        s.originals.clear();
        s.results.clear();
        let Some(text) = input.and_then(|b| core::str::from_utf8(b).ok()) else {
            return 0;
        };
        let normalizer = Normalizer::new();
        // Lines that normalise to nothing (or to too much) are skipped here,
        // because the core rejects the whole build for them.
        let items: Vec<(&str, u16)> = text
            .lines()
            .filter_map(parse_line)
            .filter(|(t, _)| {
                let n = normalizer.normalize(t);
                !n.is_empty() && n.len() <= usize::from(u16::MAX)
            })
            .collect();
        match Trie::build_normalized(&items, &normalizer) {
            Ok(trie) => {
                let n = u32::try_from(trie.len()).unwrap_or(u32::MAX);
                s.originals = (0..trie.len())
                    .map(|id| {
                        let i = trie.input_index(u32::try_from(id).unwrap_or(u32::MAX)) as usize;
                        items.get(i).map_or("", |t| t.0).into()
                    })
                    .collect();
                s.trie = Some(trie);
                n
            }
            Err(_) => 0,
        }
    })
    .unwrap_or(0)
}

/// Appends the decimal form of `n` to `out` (avoids the formatting machinery).
fn push_u64(out: &mut String, mut n: u64) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    loop {
        i -= 1;
        if let Some(d) = buf.get_mut(i) {
            *d = b'0' + (n % 10) as u8;
        }
        n /= 10;
        if n == 0 || i == 0 {
            break;
        }
    }
    for &d in buf.get(i..).unwrap_or(&[]) {
        out.push(char::from(d));
    }
}

/// Searches the index for the UTF-8 query at `(ptr, len)` (normalised like the
/// terms) and stores the results text (see the crate documentation).
/// `ranking` is 0 for `Coarse` and 1 for `Exact`. Returns the number of hits,
/// or `u32::MAX` on error.
///
/// # Safety
///
/// `ptr` must point to `len` initialised bytes (for example a buffer from
/// [`kh_alloc`]), unless `len` is 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kh_search(
    ptr: *const u8,
    len: u32,
    k: u32,
    budget: u32,
    ranking: u32,
) -> u32 {
    // SAFETY: forwarded from this function's contract.
    let input = unsafe { bytes(ptr, len) };
    with_state(|s| {
        s.results.clear();
        let Some(query) = input.and_then(|b| core::str::from_utf8(b).ok()) else {
            return ERROR;
        };
        let ranking = match ranking {
            0 => Ranking::Coarse,
            1 => Ranking::Exact,
            _ => return ERROR,
        };
        let (Ok(budget), Ok(k)) = (u16::try_from(budget), usize::try_from(k)) else {
            return ERROR;
        };
        let Some(trie) = s.trie.as_ref() else {
            return ERROR;
        };
        let cfg = SearchConfig {
            k,
            budget,
            ranking,
            ..SearchConfig::default()
        };
        let Ok(out) = s.searcher.search_text(trie, &s.costs, query, &cfg) else {
            return ERROR;
        };
        let Ok(n) = u32::try_from(out.hits.len()) else {
            return ERROR;
        };
        let r = &mut s.results;
        push_u64(r, out.stats.nodes_expanded as u64);
        r.push('\t');
        r.push(if out.stats.truncated { '1' } else { '0' });
        r.push('\n');
        for hit in &out.hits {
            let original = s.originals.get(hit.id as usize);
            r.push_str(original.map_or_else(|| trie.term(hit.id), |t| &**t));
            r.push('\t');
            push_u64(r, u64::from(hit.cost));
            r.push('\t');
            push_u64(r, u64::from(hit.weight));
            r.push('\n');
        }
        n
    })
    .unwrap_or(ERROR)
}

/// Pointer to the UTF-8 results text of the last search (see the crate
/// documentation). Valid until the next [`kh_build`] or [`kh_search`].
#[unsafe(no_mangle)]
pub extern "C" fn kh_results_ptr() -> *const u8 {
    with_state(|s| s.results.as_ptr()).unwrap_or(core::ptr::null())
}

/// Length in bytes of the results text of the last search.
#[unsafe(no_mangle)]
pub extern "C" fn kh_results_len() -> u32 {
    with_state(|s| u32::try_from(s.results.len()).unwrap_or(0)).unwrap_or(0)
}

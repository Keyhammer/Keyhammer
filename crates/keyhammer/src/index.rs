// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! A versioned, deterministic, validated binary index format and a borrowed,
//! searchable view of it.
//!
//! [`Trie::to_bytes`] writes an index; [`Index::from_bytes`] validates it once
//! and returns an [`Index`] that borrows the bytes: nothing is copied, and
//! every value is read on demand with a checked little-endian read. The view
//! can be searched directly ([`Index::search`] and friends) or turned into an
//! owned [`Trie`] ([`Index::to_trie`], [`Trie::from_bytes`]).
//!
//! Validation recomputes everything the search relies on (the tree shape, the
//! labels, the term ids and the per-node bounds), so an accepted index is
//! exactly the trie that [`Trie::build`] makes from its own terms, and a
//! search on it terminates, never panics and returns the results of that
//! trie. The layout, the checks and the compatibility rules are specified in
//! `docs/design/index-format.md`.
//!
//! ```
//! use keyhammer::cost::CostModel;
//! use keyhammer::index::Index;
//! use keyhammer::search::{SearchConfig, Searcher};
//! use keyhammer::trie::Trie;
//!
//! let trie = Trie::build(&[("javascript", 10), ("java", 30), ("python", 50)]).unwrap();
//! let bytes = trie.to_bytes().unwrap();
//! // Later, possibly in another process: validate once, then search the bytes.
//! let index = Index::from_bytes(&bytes).unwrap();
//! let mut searcher = Searcher::new();
//! let cfg = SearchConfig::default();
//! let out = index
//!     .search(&mut searcher, &CostModel::qwerty(), b"javasript", &cfg)
//!     .unwrap();
//! assert_eq!(index.term(out.hits[0].id), "javascript");
//! // The same results as the in-memory trie.
//! let owned = searcher.search(&trie, &CostModel::qwerty(), b"javasript", &cfg).unwrap();
//! assert_eq!(out.hits, owned.hits);
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::ops::Range;

use crate::cost::{CostModel, symbol_class};
use crate::search::{Output, SearchConfig, SearchError, Searcher};
use crate::text::{self, Normalizer};
use crate::trie::{NO_TERM, Nodes, Parts, Trie};

/// The first eight bytes of every index: `"KHINDEX\0"`.
pub const MAGIC: [u8; 8] = *b"KHINDEX\0";
/// The format version this crate writes and the only one it reads.
pub const VERSION: u16 = 1;
/// The layout profile this crate writes and the only one it reads.
pub const PROFILE: u16 = 1;

const HEADER_LEN: usize = 64;
const ENTRY_LEN: usize = 24;
const SECTIONS: usize = 11;
/// End of the section table, where the first section starts.
const TABLE_END: usize = HEADER_LEN + SECTIONS * ENTRY_LEN;
/// Offset of the CRC field, read as zero while the CRC is computed.
const CRC_AT: usize = 24;
const FLAG_NORMALIZER: u32 = 1;
const MODE_CASE: u8 = 1;
const MODE_DIACRITICS: u8 = 2;
/// Longest term, in bytes, as in [`Trie::build`].
const MAX_TERM_BYTES: usize = u16::MAX as usize;

// Section positions (the section id is the position plus one).
const LABELS: usize = 0;
const CHILDREN: usize = 1;
const TERM_IDS: usize = 2;
const LEN_MIN: usize = 3;
const LEN_MAX: usize = 4;
const MAX_WEIGHT: usize = 5;
const BELOW_MASK: usize = 6;
const WEIGHTS: usize = 7;
const INPUT_INDEX: usize = 8;
const TERM_OFFSETS: usize = 9;
const POOL: usize = 10;

/// Element size of each section, in bytes.
const ELEM: [u64; SECTIONS] = [4, 4, 4, 2, 2, 2, 8, 2, 4, 4, 1];

/// Why an index could not be written or loaded. The checks run in the order
/// of `docs/design/index-format.md`, section 5, and the first failure is
/// returned; `node` and `term` fields name the offending node or term id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FormatError {
    /// The input is shorter than the 64-byte header.
    TooShort {
        /// Length of the input.
        len: usize,
    },
    /// The input does not start with [`MAGIC`].
    BadMagic,
    /// The format version is not [`VERSION`] (newer or older).
    UnsupportedVersion {
        /// The version in the header.
        found: u16,
    },
    /// The profile id is not [`PROFILE`].
    UnknownProfile {
        /// The profile in the header.
        found: u16,
    },
    /// Flag bits this version does not define are set.
    UnknownFlags {
        /// The flags in the header.
        flags: u32,
    },
    /// The length in the header differs from the length of the input
    /// (a truncated or extended file).
    LengthMismatch {
        /// The length in the header.
        declared: u64,
        /// Length of the input.
        actual: usize,
    },
    /// The CRC-32 does not match the contents.
    ChecksumMismatch {
        /// The CRC in the header.
        stored: u32,
        /// The CRC of the contents.
        computed: u32,
    },
    /// A reserved header field or a padding byte is not zero.
    NonZeroReserved {
        /// Offset of the first non-zero byte.
        offset: usize,
    },
    /// The normaliser mode byte has bits other than case and diacritics.
    BadNormalizer {
        /// The mode byte.
        modes: u8,
    },
    /// The index was built with another version of the normaliser
    /// (algorithm or Unicode tables) than this crate's: rebuild it.
    NormalizerMismatch {
        /// The algorithm version in the header.
        algorithm: u16,
        /// The Unicode version (major, minor, update) in the header.
        unicode: [u8; 3],
    },
    /// The section count is not the one of this version.
    SectionCount {
        /// The count in the header.
        found: u32,
    },
    /// The node, term or string pool counts are impossible or disagree with
    /// the file length.
    BadCounts,
    /// A section table entry differs from the layout the counts imply.
    BadSectionTable {
        /// Section id (1 to 11).
        section: u32,
    },
    /// The child ranges are not contiguous, breadth first, after their parent
    /// and at most 65535 long.
    BadChildren {
        /// The node.
        node: u32,
    },
    /// A node is deeper than 65535.
    TooDeep {
        /// The node.
        node: u32,
    },
    /// A label is not a Unicode scalar value, the root's is not 0, or the
    /// labels of siblings are not strictly increasing.
    BadLabel {
        /// The node.
        node: u32,
    },
    /// A term id is out of range, on the root, or missing on a leaf.
    BadTermId {
        /// The node.
        node: u32,
    },
    /// The number of nodes with a term differs from the term count.
    TerminalCount {
        /// Nodes with a term.
        found: u32,
        /// The term count.
        expected: u32,
    },
    /// A stored `len_min` or `len_max` differs from the recomputed value.
    BadLenBounds {
        /// The node.
        node: u32,
    },
    /// A stored `below_mask` differs from the recomputed value.
    BadBelowMask {
        /// The node.
        node: u32,
    },
    /// A stored `max_weight` differs from the recomputed value.
    BadMaxWeight {
        /// The node.
        node: u32,
    },
    /// The term offsets are not increasing from 0 to the pool length, or a
    /// term is empty or longer than 65535 bytes.
    BadTermOffsets {
        /// The term id.
        term: u32,
    },
    /// A term is not valid UTF-8.
    InvalidUtf8 {
        /// The term id.
        term: u32,
    },
    /// A term is not strictly after the previous one in byte order.
    TermsNotSorted {
        /// The term id.
        term: u32,
    },
    /// A term's path does not lead to a node with its id.
    TermNotInTrie {
        /// The term id.
        term: u32,
    },
    /// A term is not in the normal form of the recorded normaliser.
    TermNotNormalized {
        /// The term id.
        term: u32,
    },
    /// An input index is `u32::MAX`.
    BadInputIndex {
        /// The term id.
        term: u32,
    },
    /// The trie exceeds the limits of the format (writing only).
    TooLarge,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use FormatError as E;
        match *self {
            E::TooShort { len } => write!(f, "index of {len} bytes is shorter than its header"),
            E::BadMagic => f.write_str("not a keyhammer index (bad magic)"),
            E::UnsupportedVersion { found } => {
                write!(
                    f,
                    "index format version {found} is not supported (expected {VERSION})"
                )
            }
            E::UnknownProfile { found } => write!(f, "unknown index profile {found}"),
            E::UnknownFlags { flags } => write!(f, "unknown index flags {flags:#x}"),
            E::LengthMismatch { declared, actual } => {
                write!(f, "index declares {declared} bytes but has {actual}")
            }
            E::ChecksumMismatch { stored, computed } => {
                write!(
                    f,
                    "index checksum {stored:#010x} does not match {computed:#010x}"
                )
            }
            E::NonZeroReserved { offset } => {
                write!(f, "reserved or padding byte at offset {offset} is not zero")
            }
            E::BadNormalizer { modes } => write!(f, "unknown normaliser modes {modes:#x}"),
            E::NormalizerMismatch { algorithm, unicode } => write!(
                f,
                "index was built with normaliser version {algorithm} (Unicode {}.{}.{}), \
                 this build has another; rebuild the index",
                unicode[0], unicode[1], unicode[2]
            ),
            E::SectionCount { found } => write!(f, "index has {found} sections, expected 11"),
            E::BadCounts => f.write_str("index node, term or text counts are invalid"),
            E::BadSectionTable { section } => write!(f, "section table entry {section} is invalid"),
            E::BadChildren { node } => write!(f, "invalid child range at node {node}"),
            E::TooDeep { node } => write!(f, "node {node} is deeper than 65535"),
            E::BadLabel { node } => write!(f, "invalid label at node {node}"),
            E::BadTermId { node } => write!(f, "invalid term id at node {node}"),
            E::TerminalCount { found, expected } => {
                write!(f, "{found} nodes carry a term, expected {expected}")
            }
            E::BadLenBounds { node } => write!(f, "wrong length bounds at node {node}"),
            E::BadBelowMask { node } => write!(f, "wrong symbol-class mask at node {node}"),
            E::BadMaxWeight { node } => write!(f, "wrong maximum weight at node {node}"),
            E::BadTermOffsets { term } => write!(f, "invalid text offsets for term {term}"),
            E::InvalidUtf8 { term } => write!(f, "term {term} is not valid UTF-8"),
            E::TermsNotSorted { term } => write!(f, "term {term} is out of order"),
            E::TermNotInTrie { term } => write!(f, "term {term} does not match the trie"),
            E::TermNotNormalized { term } => write!(f, "term {term} is not normalised"),
            E::BadInputIndex { term } => write!(f, "invalid input index for term {term}"),
            E::TooLarge => f.write_str("the trie exceeds the limits of the index format"),
        }
    }
}

// ---------------------------------------------------------------------------
// CRC-32 (IEEE 802.3, reflected, as zlib), slicing by 8.

const fn crc_tables() -> [[u32; 256]; 8] {
    let mut t = [[0u32; 256]; 8];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        t[0][i] = c;
        i += 1;
    }
    let mut i = 0;
    while i < 256 {
        let mut j = 1;
        while j < 8 {
            let p = t[j - 1][i];
            t[j][i] = (p >> 8) ^ t[0][(p & 0xFF) as usize];
            j += 1;
        }
        i += 1;
    }
    t
}

static CRC: [[u32; 256]; 8] = crc_tables();

/// Feeds `data` to a running (pre-inverted) CRC-32 state.
fn crc_update(mut c: u32, data: &[u8]) -> u32 {
    let mut chunks = data.chunks_exact(8);
    for ch in &mut chunks {
        let lo = c ^ u32::from_le_bytes([ch[0], ch[1], ch[2], ch[3]]);
        let hi = u32::from_le_bytes([ch[4], ch[5], ch[6], ch[7]]);
        c = CRC[7][(lo & 0xFF) as usize]
            ^ CRC[6][((lo >> 8) & 0xFF) as usize]
            ^ CRC[5][((lo >> 16) & 0xFF) as usize]
            ^ CRC[4][(lo >> 24) as usize]
            ^ CRC[3][(hi & 0xFF) as usize]
            ^ CRC[2][((hi >> 8) & 0xFF) as usize]
            ^ CRC[1][((hi >> 16) & 0xFF) as usize]
            ^ CRC[0][(hi >> 24) as usize];
    }
    for &b in chunks.remainder() {
        c = CRC[0][((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
    }
    c
}

/// The CRC of a whole index, with the CRC field read as zero.
fn file_crc(bytes: &[u8]) -> u32 {
    let (head, rest) = bytes.split_at(CRC_AT.min(bytes.len()));
    let tail = rest.get(4..).unwrap_or(&[]);
    !crc_update(crc_update(crc_update(!0, head), &[0; 4]), tail)
}

// ---------------------------------------------------------------------------
// Checked little-endian reads: out of range reads return 0 (never reached for
// a validated index, whose ranges are all checked first).

#[inline]
fn slot(len: usize, i: usize, size: usize) -> Option<Range<usize>> {
    let a = i.checked_mul(size)?;
    let b = a.checked_add(size)?;
    (b <= len).then_some(a..b)
}

#[inline]
fn rd_u16(s: &[u8], i: usize) -> u16 {
    match slot(s.len(), i, 2).and_then(|r| s.get(r)) {
        Some(&[a, b]) => u16::from_le_bytes([a, b]),
        _ => 0,
    }
}

#[inline]
fn rd_u32(s: &[u8], i: usize) -> u32 {
    match slot(s.len(), i, 4).and_then(|r| s.get(r)) {
        Some(&[a, b, c, d]) => u32::from_le_bytes([a, b, c, d]),
        _ => 0,
    }
}

#[inline]
fn rd_u64(s: &[u8], i: usize) -> u64 {
    match slot(s.len(), i, 8).and_then(|r| s.get(r)) {
        Some(&[a, b, c, d, e, f, g, h]) => u64::from_le_bytes([a, b, c, d, e, f, g, h]),
        _ => 0,
    }
}

/// Reads a `u16` at byte offset `off` (not an element index).
fn at_u16(s: &[u8], off: usize) -> u16 {
    match s.get(off..off.saturating_add(2)) {
        Some(&[a, b]) => u16::from_le_bytes([a, b]),
        _ => 0,
    }
}

fn at_u32(s: &[u8], off: usize) -> u32 {
    match s.get(off..off.saturating_add(4)) {
        Some(&[a, b, c, d]) => u32::from_le_bytes([a, b, c, d]),
        _ => 0,
    }
}

fn at_u64(s: &[u8], off: usize) -> u64 {
    match s.get(off..off.saturating_add(8)) {
        Some(&[a, b, c, d, e, f, g, h]) => u64::from_le_bytes([a, b, c, d, e, f, g, h]),
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Layout.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Section {
    offset: u64,
    len: u64,
}

fn round8(x: u64) -> u64 {
    (x + 7) & !7
}

/// The sections and total length implied by `n` nodes, `t` terms and a pool
/// of `p` bytes. Every count is at most `2^32`, so nothing here overflows.
fn layout(n: u32, t: u32, p: u32) -> ([Section; SECTIONS], u64) {
    let (n, t, p) = (u64::from(n), u64::from(t), u64::from(p));
    let counts = [n, n + 1, n, n, n, n, n, t, t, t + 1, p];
    let mut out = [Section { offset: 0, len: 0 }; SECTIONS];
    let mut at = TABLE_END as u64;
    for (i, s) in out.iter_mut().enumerate() {
        *s = Section {
            offset: at,
            len: counts[i] * ELEM[i],
        };
        at = round8(at + s.len);
    }
    (out, at)
}

// ---------------------------------------------------------------------------
// Writer.

fn put_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Writes `trie` in format version 1 (see [`Trie::to_bytes`]).
pub(crate) fn write(trie: &Trie) -> Result<Vec<u8>, FormatError> {
    let too_large = |_| FormatError::TooLarge;
    let n_nodes = trie.node_count();
    let n_terms = trie.len();
    let n = u32::try_from(n_nodes).map_err(too_large)?;
    let t = u32::try_from(n_terms).map_err(too_large)?;
    let pool_len: usize = (0..t).map(|id| trie.term(id).len()).sum();
    let p = u32::try_from(pool_len).map_err(too_large)?;
    if t == NO_TERM {
        return Err(FormatError::TooLarge);
    }
    let (sections, total) = layout(n, t, p);
    let total = usize::try_from(total).map_err(too_large)?;

    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&MAGIC);
    put_u16(&mut out, VERSION);
    put_u16(&mut out, PROFILE);
    let norm = trie.normalizer();
    put_u32(&mut out, if norm.is_some() { FLAG_NORMALIZER } else { 0 });
    put_u64(&mut out, total as u64);
    put_u32(&mut out, 0); // CRC, filled in last
    put_u32(&mut out, SECTIONS as u32);
    put_u32(&mut out, n);
    put_u32(&mut out, t);
    put_u32(&mut out, p);
    match norm {
        Some(nz) => {
            let mut modes = 0;
            if nz.folds_case() {
                modes |= MODE_CASE;
            }
            if nz.folds_diacritics() {
                modes |= MODE_DIACRITICS;
            }
            out.extend_from_slice(&[modes, 0]);
            put_u16(&mut out, text::ALGORITHM_VERSION);
            out.extend_from_slice(&text::UNICODE_VERSION);
            out.push(0);
        }
        None => out.extend_from_slice(&[0; 8]),
    }
    out.extend_from_slice(&[0; 12]);
    for (i, s) in sections.iter().enumerate() {
        put_u32(&mut out, i as u32 + 1);
        put_u32(&mut out, ELEM[i] as u32);
        put_u64(&mut out, s.offset);
        put_u64(&mut out, s.len);
    }

    for (i, s) in sections.iter().enumerate() {
        out.resize(s.offset as usize, 0);
        match i {
            LABELS => (0..n_nodes).for_each(|v| put_u32(&mut out, Nodes::symbol(trie, v))),
            CHILDREN => {
                (0..n_nodes).for_each(|v| put_u32(&mut out, trie.children(v).start as u32));
                put_u32(&mut out, n);
            }
            TERM_IDS => (0..n_nodes).for_each(|v| put_u32(&mut out, trie.term_id(v))),
            LEN_MIN => (0..n_nodes).for_each(|v| put_u16(&mut out, trie.len_min(v))),
            LEN_MAX => (0..n_nodes).for_each(|v| put_u16(&mut out, trie.len_max(v))),
            MAX_WEIGHT => (0..n_nodes).for_each(|v| put_u16(&mut out, trie.max_weight(v))),
            BELOW_MASK => (0..n_nodes).for_each(|v| put_u64(&mut out, trie.below_mask(v))),
            WEIGHTS => (0..t).for_each(|id| put_u16(&mut out, trie.weight(id))),
            INPUT_INDEX => (0..t).for_each(|id| put_u32(&mut out, trie.input_index(id))),
            TERM_OFFSETS => {
                let mut at = 0u32;
                put_u32(&mut out, 0);
                for id in 0..t {
                    // The sum of all lengths fits in u32 (checked above).
                    at += trie.term(id).len() as u32;
                    put_u32(&mut out, at);
                }
            }
            _ => (0..t).for_each(|id| out.extend_from_slice(trie.term(id).as_bytes())),
        }
    }
    out.resize(total, 0);
    let crc = file_crc(&out);
    out[CRC_AT..CRC_AT + 4].copy_from_slice(&crc.to_le_bytes());
    Ok(out)
}

// ---------------------------------------------------------------------------
// The view.

/// A validated, borrowed view of a serialized index.
///
/// Built by [`Index::from_bytes`], which checks everything once (see the
/// [module documentation](self)); afterwards every read is a checked
/// little-endian read from the borrowed bytes, and nothing is copied. Search
/// it with [`Index::search`], [`Index::search_prefix`], [`Index::search_text`]
/// and [`Index::search_prefix_text`], which return exactly what the same
/// `Searcher` methods return on the trie that was written, or convert it with
/// [`Index::to_trie`].
///
/// Node ids, term ids and every accessor mean what they mean on [`Trie`]; the
/// accessors return `0`, `'\0'`, `""`, an empty range or [`NO_TERM`] for an
/// out-of-range argument instead of panicking.
#[derive(Clone, Copy)]
pub struct Index<'a> {
    bytes: &'a [u8],
    sec: [&'a [u8]; SECTIONS],
    nodes: u32,
    terms: u32,
    normalizer: Option<Normalizer>,
}

impl fmt::Debug for Index<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Index")
            .field("bytes", &self.bytes.len())
            .field("nodes", &self.nodes)
            .field("terms", &self.terms)
            .field("normalizer", &self.normalizer)
            .finish()
    }
}

impl<'a> Index<'a> {
    /// Validates `bytes` as an index and returns a view of them.
    ///
    /// Runs every check of `docs/design/index-format.md`, section 5, in that
    /// order, and returns the first failure: header, length and CRC-32; the
    /// tree (contiguous breadth-first child ranges, each after its parent,
    /// valid and sorted labels, term ids); every per-node bound recomputed
    /// and compared; the terms (UTF-8, sorted, each found in the trie at its
    /// id, normalised if a normaliser is recorded). Linear in the input; it
    /// never panics and allocates nothing. The input may start at any address.
    ///
    /// ```
    /// use keyhammer::index::{FormatError, Index};
    /// use keyhammer::trie::Trie;
    ///
    /// let bytes = Trie::build(&[("car", 9), ("cat", 4)]).unwrap().to_bytes().unwrap();
    /// let index = Index::from_bytes(&bytes).unwrap();
    /// assert_eq!(index.len(), 2);
    /// assert_eq!(index.term(0), "car");
    ///
    /// let mut bad = bytes.clone();
    /// bad[100] ^= 1;
    /// assert!(matches!(
    ///     Index::from_bytes(&bad),
    ///     Err(FormatError::ChecksumMismatch { .. })
    /// ));
    /// assert!(Index::from_bytes(&bytes[..bytes.len() - 8]).is_err());
    /// ```
    pub fn from_bytes(bytes: &'a [u8]) -> Result<Index<'a>, FormatError> {
        let (ix, n, t) = header(bytes)?;
        ix.check_tree(n, t)?;
        ix.check_terms(t)?;
        Ok(ix)
    }

    /// The bytes the view borrows (the whole index).
    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Copies the view into an owned [`Trie`], equal to the trie that was
    /// written.
    pub fn to_trie(&self) -> Trie {
        let n = self.node_count();
        let t = self.terms;
        Trie::from_parts(Parts {
            terms: (0..t).map(|id| String::from(self.term(id))).collect(),
            weights: (0..t).map(|id| self.weight(id)).collect(),
            input_index: (0..t).map(|id| self.input_index(id)).collect(),
            labels: (0..n).map(|v| self.symbol(v)).collect(),
            child_start: (0..n).map(|v| self.children(v).start as u32).collect(),
            child_count: (0..n).map(|v| self.children(v).len() as u16).collect(),
            term_id: (0..n).map(|v| self.term_id(v)).collect(),
            max_weight: (0..n).map(|v| self.max_weight(v)).collect(),
            len_min: (0..n).map(|v| self.len_min(v)).collect(),
            len_max: (0..n).map(|v| self.len_max(v)).collect(),
            below_mask: (0..n).map(|v| self.below_mask(v)).collect(),
            normalizer: self.normalizer,
        })
    }

    /// Number of distinct terms.
    pub fn len(&self) -> usize {
        self.terms as usize
    }

    /// Whether the index holds no terms (never true for a valid index).
    pub fn is_empty(&self) -> bool {
        self.terms == 0
    }

    /// Number of trie nodes, root included.
    pub fn node_count(&self) -> usize {
        self.nodes as usize
    }

    /// The normaliser the trie was built with, as [`Trie::normalizer`].
    pub fn normalizer(&self) -> Option<Normalizer> {
        self.normalizer
    }

    /// The term with the given id (`""` for an unknown id). The text is
    /// borrowed from the index; it was validated as UTF-8 at load and is
    /// checked again on each call, which is linear in its length.
    pub fn term(&self, id: u32) -> &'a str {
        let off = self.sec[TERM_OFFSETS];
        let a = rd_u32(off, id as usize) as usize;
        let b = rd_u32(off, (id as usize).saturating_add(1)) as usize;
        match self.sec[POOL].get(a..b) {
            Some(s) if id < self.terms => core::str::from_utf8(s).unwrap_or(""),
            _ => "",
        }
    }

    /// The weight of the term with the given id (0 for an unknown id).
    pub fn weight(&self, id: u32) -> u16 {
        rd_u16(self.sec[WEIGHTS], id as usize)
    }

    /// As [`Trie::input_index`]: `u32::MAX` for an unknown id.
    pub fn input_index(&self, id: u32) -> u32 {
        if id < self.terms {
            rd_u32(self.sec[INPUT_INDEX], id as usize)
        } else {
            u32::MAX
        }
    }

    /// The symbol on the edge into node `v` (`'\0'` for the root or an
    /// unknown node).
    pub fn label(&self, v: usize) -> char {
        char::from_u32(self.symbol(v)).unwrap_or('\0')
    }

    #[inline]
    fn symbol(&self, v: usize) -> u32 {
        rd_u32(self.sec[LABELS], v)
    }

    /// The node indices of the children of `v` (empty for an unknown node).
    #[inline]
    pub fn children(&self, v: usize) -> Range<usize> {
        if v >= self.node_count() {
            return 0..0;
        }
        let c = self.sec[CHILDREN];
        rd_u32(c, v) as usize..rd_u32(c, v + 1) as usize
    }

    /// The id of the term that ends at `v`, or [`NO_TERM`].
    #[inline]
    pub fn term_id(&self, v: usize) -> u32 {
        if v < self.node_count() {
            rd_u32(self.sec[TERM_IDS], v)
        } else {
            NO_TERM
        }
    }

    /// The largest weight among the terms at or below `v`.
    #[inline]
    pub fn max_weight(&self, v: usize) -> u16 {
        rd_u16(self.sec[MAX_WEIGHT], v)
    }

    /// The length, in symbols, of the shortest term at or below `v`.
    #[inline]
    pub fn len_min(&self, v: usize) -> u16 {
        rd_u16(self.sec[LEN_MIN], v)
    }

    /// The length, in symbols, of the longest term at or below `v`.
    #[inline]
    pub fn len_max(&self, v: usize) -> u16 {
        rd_u16(self.sec[LEN_MAX], v)
    }

    /// The symbol classes that appear on edges strictly below `v`.
    #[inline]
    pub fn below_mask(&self, v: usize) -> u64 {
        rd_u64(self.sec[BELOW_MASK], v)
    }

    /// [`Searcher::search`] on the view: the same hits, costs and work
    /// counters as on the trie that was written.
    pub fn search(
        &self,
        searcher: &mut Searcher,
        cm: &CostModel,
        q: &[u8],
        cfg: &SearchConfig,
    ) -> Result<Output, SearchError> {
        searcher.search_in(self, cm, q, cfg, false)
    }

    /// [`Searcher::search_prefix`] on the view.
    pub fn search_prefix(
        &self,
        searcher: &mut Searcher,
        cm: &CostModel,
        q: &[u8],
        cfg: &SearchConfig,
    ) -> Result<Output, SearchError> {
        searcher.search_in(self, cm, q, cfg, true)
    }

    /// [`Searcher::search_text`] on the view: `q` is normalised with the
    /// recorded normaliser, if any.
    pub fn search_text(
        &self,
        searcher: &mut Searcher,
        cm: &CostModel,
        q: &str,
        cfg: &SearchConfig,
    ) -> Result<Output, SearchError> {
        searcher.search_text_in(self, cm, q, cfg, false)
    }

    /// [`Searcher::search_prefix_text`] on the view.
    pub fn search_prefix_text(
        &self,
        searcher: &mut Searcher,
        cm: &CostModel,
        q: &str,
        cfg: &SearchConfig,
    ) -> Result<Output, SearchError> {
        searcher.search_text_in(self, cm, q, cfg, true)
    }

    /// The child of `v` labelled `sym`, by binary search among the sorted
    /// labels of its children.
    fn find_child(&self, v: usize, sym: u32) -> Option<usize> {
        let r = self.children(v);
        let (mut lo, mut hi) = (r.start, r.end);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match self.symbol(mid).cmp(&sym) {
                core::cmp::Ordering::Less => lo = mid + 1,
                core::cmp::Ordering::Greater => hi = mid,
                core::cmp::Ordering::Equal => return Some(mid),
            }
        }
        None
    }

    /// Section 5.2 of the design note: children, depth, labels, term ids and
    /// the recomputed aggregates, in one pass without allocating.
    fn check_tree(&self, n: u32, t: u32) -> Result<(), FormatError> {
        let cs = self.sec[CHILDREN];
        let nn = n as usize;
        if rd_u32(cs, 0) != 1 {
            return Err(FormatError::BadChildren { node: 0 });
        }
        if rd_u32(cs, nn) != n {
            return Err(FormatError::BadChildren { node: n });
        }
        if self.symbol(0) != 0 {
            return Err(FormatError::BadLabel { node: 0 });
        }
        if self.term_id(0) != NO_TERM {
            return Err(FormatError::BadTermId { node: 0 });
        }
        // Level L occupies s_L..s_{L+1}; s_{L+2} = the first child of s_{L+1}.
        let mut depth: u32 = 0;
        let mut level_end: usize = 1;
        let mut terminals: u32 = 0;
        for v in 0..nn {
            let node = v as u32;
            let (a, b) = (rd_u32(cs, v) as usize, rd_u32(cs, v + 1) as usize);
            if b < a || b - a > usize::from(u16::MAX) || a <= v {
                return Err(FormatError::BadChildren { node });
            }
            if v == level_end {
                depth += 1;
                level_end = a;
            }
            let depth16 = u16::try_from(depth).map_err(|_| FormatError::TooDeep { node })?;
            // The labels of the children: scalar values, strictly increasing.
            // Checked here, before the aggregates of `v` read them.
            for c in a..b {
                let sym = self.symbol(c);
                if char::from_u32(sym).is_none() || (c > a && sym <= self.symbol(c - 1)) {
                    return Err(FormatError::BadLabel { node: c as u32 });
                }
            }
            let tid = self.term_id(v);
            let (mut lmin, mut lmax, mut w, mut mask) = (u16::MAX, 0u16, 0u16, 0u64);
            if tid != NO_TERM {
                if tid >= t {
                    return Err(FormatError::BadTermId { node });
                }
                terminals += 1;
                (lmin, lmax, w) = (depth16, depth16, self.weight(tid));
            } else if a == b {
                // A leaf without a term: `Trie::build` never makes one.
                return Err(FormatError::BadTermId { node });
            }
            for c in a..b {
                lmin = lmin.min(self.len_min(c));
                lmax = lmax.max(self.len_max(c));
                w = w.max(self.max_weight(c));
                mask |= symbol_class(self.symbol(c)) | self.below_mask(c);
            }
            if self.len_min(v) != lmin || self.len_max(v) != lmax {
                return Err(FormatError::BadLenBounds { node });
            }
            if self.below_mask(v) != mask {
                return Err(FormatError::BadBelowMask { node });
            }
            if self.max_weight(v) != w {
                return Err(FormatError::BadMaxWeight { node });
            }
        }
        if terminals != t {
            return Err(FormatError::TerminalCount {
                found: terminals,
                expected: t,
            });
        }
        Ok(())
    }

    /// Section 5.3 of the design note: offsets, UTF-8, order, each term at
    /// its id in the trie, normal form, input indices.
    fn check_terms(&self, t: u32) -> Result<(), FormatError> {
        let off = self.sec[TERM_OFFSETS];
        let pool = self.sec[POOL];
        if rd_u32(off, 0) != 0 {
            return Err(FormatError::BadTermOffsets { term: 0 });
        }
        if rd_u32(off, t as usize) as usize != pool.len() {
            return Err(FormatError::BadTermOffsets { term: t });
        }
        let mut prev: &[u8] = &[];
        for term in 0..t {
            let i = term as usize;
            let (a, b) = (rd_u32(off, i) as usize, rd_u32(off, i + 1) as usize);
            if b <= a || b - a > MAX_TERM_BYTES {
                return Err(FormatError::BadTermOffsets { term });
            }
            let raw = pool.get(a..b).ok_or(FormatError::BadTermOffsets { term })?;
            let s = core::str::from_utf8(raw).map_err(|_| FormatError::InvalidUtf8 { term })?;
            if term > 0 && raw <= prev {
                return Err(FormatError::TermsNotSorted { term });
            }
            prev = raw;
            let mut v = 0;
            for c in s.chars() {
                v = self
                    .find_child(v, u32::from(c))
                    .ok_or(FormatError::TermNotInTrie { term })?;
            }
            if self.term_id(v) != term {
                return Err(FormatError::TermNotInTrie { term });
            }
            if let Some(nz) = self.normalizer {
                if !text::is_normalized(nz, s) {
                    return Err(FormatError::TermNotNormalized { term });
                }
            }
            if self.input_index(term) == u32::MAX {
                return Err(FormatError::BadInputIndex { term });
            }
        }
        Ok(())
    }
}

/// Section 5.1 of the design note: header, length, CRC, reserved fields,
/// normaliser, counts and section table. Returns the view with its sections
/// and the node and term counts.
fn header(bytes: &[u8]) -> Result<(Index<'_>, u32, u32), FormatError> {
    if bytes.len() < HEADER_LEN {
        return Err(FormatError::TooShort { len: bytes.len() });
    }
    if bytes.get(..8) != Some(&MAGIC[..]) {
        return Err(FormatError::BadMagic);
    }
    let version = at_u16(bytes, 8);
    if version != VERSION {
        return Err(FormatError::UnsupportedVersion { found: version });
    }
    let profile = at_u16(bytes, 10);
    if profile != PROFILE {
        return Err(FormatError::UnknownProfile { found: profile });
    }
    let flags = at_u32(bytes, 12);
    if flags & !FLAG_NORMALIZER != 0 {
        return Err(FormatError::UnknownFlags { flags });
    }
    let declared = at_u64(bytes, 16);
    if declared != bytes.len() as u64 {
        return Err(FormatError::LengthMismatch {
            declared,
            actual: bytes.len(),
        });
    }
    let stored = at_u32(bytes, CRC_AT);
    let computed = file_crc(bytes);
    if stored != computed {
        return Err(FormatError::ChecksumMismatch { stored, computed });
    }
    let zero_at = |r: Range<usize>| -> Result<(), FormatError> {
        match bytes
            .get(r.clone())
            .and_then(|s| s.iter().position(|&x| x != 0))
        {
            Some(i) => Err(FormatError::NonZeroReserved {
                offset: r.start + i,
            }),
            None => Ok(()),
        }
    };
    zero_at(45..46)?;
    zero_at(51..HEADER_LEN)?;
    let normalizer = if flags & FLAG_NORMALIZER != 0 {
        let modes = bytes.get(44).copied().unwrap_or(0);
        if modes & !(MODE_CASE | MODE_DIACRITICS) != 0 {
            return Err(FormatError::BadNormalizer { modes });
        }
        let algorithm = at_u16(bytes, 46);
        let unicode = match bytes.get(48..51) {
            Some(&[a, b, c]) => [a, b, c],
            _ => [0; 3],
        };
        if algorithm != text::ALGORITHM_VERSION || unicode != text::UNICODE_VERSION {
            return Err(FormatError::NormalizerMismatch { algorithm, unicode });
        }
        Some(
            Normalizer::new()
                .with_case_folding(modes & MODE_CASE != 0)
                .with_diacritic_folding(modes & MODE_DIACRITICS != 0),
        )
    } else {
        zero_at(44..51)?;
        None
    };
    let count = at_u32(bytes, 28);
    if count as usize != SECTIONS {
        return Err(FormatError::SectionCount { found: count });
    }
    let (n, t, p) = (at_u32(bytes, 32), at_u32(bytes, 36), at_u32(bytes, 40));
    if n < 2 || t == 0 || t > n - 1 || p < t {
        return Err(FormatError::BadCounts);
    }
    let (sections, total) = layout(n, t, p);
    let mut sec: [&[u8]; SECTIONS] = [&[]; SECTIONS];
    for (i, s) in sections.iter().enumerate() {
        let e = HEADER_LEN + i * ENTRY_LEN;
        let entry_ok = at_u32(bytes, e) == i as u32 + 1
            && u64::from(at_u32(bytes, e + 4)) == ELEM[i]
            && at_u64(bytes, e + 8) == s.offset
            && at_u64(bytes, e + 16) == s.len;
        if !entry_ok {
            return Err(FormatError::BadSectionTable {
                section: i as u32 + 1,
            });
        }
    }
    if total != bytes.len() as u64 {
        return Err(FormatError::BadCounts);
    }
    // Every offset below is at most `total`, the length of `bytes`, so the
    // conversions to usize are exact.
    for (i, s) in sections.iter().enumerate() {
        let (a, b) = (s.offset as usize, (s.offset + s.len) as usize);
        sec[i] = bytes.get(a..b).ok_or(FormatError::BadCounts)?;
        zero_at(b..round8(s.offset + s.len) as usize)?;
    }
    let ix = Index {
        bytes,
        sec,
        nodes: n,
        terms: t,
        normalizer,
    };
    Ok((ix, n, t))
}

impl Nodes for Index<'_> {
    #[inline]
    fn symbol(&self, v: usize) -> u32 {
        Index::symbol(self, v)
    }
    #[inline]
    fn children(&self, v: usize) -> Range<usize> {
        Index::children(self, v)
    }
    #[inline]
    fn term_id(&self, v: usize) -> u32 {
        Index::term_id(self, v)
    }
    #[inline]
    fn max_weight(&self, v: usize) -> u16 {
        Index::max_weight(self, v)
    }
    #[inline]
    fn len_min(&self, v: usize) -> u16 {
        Index::len_min(self, v)
    }
    #[inline]
    fn len_max(&self, v: usize) -> u16 {
        Index::len_max(self, v)
    }
    #[inline]
    fn below_mask(&self, v: usize) -> u64 {
        Index::below_mask(self, v)
    }
    #[inline]
    fn weight(&self, id: u32) -> u16 {
        Index::weight(self, id)
    }
    #[inline]
    fn normalizer(&self) -> Option<Normalizer> {
        self.normalizer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The CRC-32 of `data` (zlib's `crc32`).
    fn crc32(data: &[u8]) -> u32 {
        !crc_update(!0, data)
    }

    /// Bit-by-bit reference CRC-32.
    fn crc_slow(data: &[u8]) -> u32 {
        let mut c = !0u32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }

    #[test]
    fn crc32_matches_the_known_vectors() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"a"), 0xE8B7_BE43);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(
            crc32(b"The quick brown fox jumps over the lazy dog"),
            0x414F_A339
        );
    }

    #[test]
    fn sliced_crc32_equals_the_bitwise_reference_at_every_length_and_split() {
        let data: Vec<u8> = (0..300u32).map(|i| (i * 7 + i / 5) as u8).collect();
        let lens = if cfg!(miri) { 40 } else { data.len() };
        for len in 0..lens {
            let d = &data[..len];
            assert_eq!(crc32(d), crc_slow(d), "length {len}");
            // Feeding in two pieces gives the same result.
            let mid = len / 3;
            assert_eq!(
                !crc_update(crc_update(!0, &d[..mid]), &d[mid..]),
                crc_slow(d)
            );
        }
    }

    #[test]
    fn layout_is_aligned_monotonic_and_ends_at_the_total() {
        for (n, t, p) in [(2, 1, 1), (5, 2, 6), (7, 3, 11), (1000, 400, 3001)] {
            let (s, total) = layout(n, t, p);
            assert_eq!(s[0].offset, TABLE_END as u64);
            for w in s.windows(2) {
                assert_eq!(w[1].offset, round8(w[0].offset + w[0].len));
            }
            assert!(s.iter().all(|x| x.offset % 8 == 0));
            assert_eq!(total, round8(s[SECTIONS - 1].offset + s[SECTIONS - 1].len));
        }
        assert_eq!(TABLE_END % 8, 0);
    }
}

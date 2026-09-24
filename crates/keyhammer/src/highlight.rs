// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! Match highlighting: which characters of a hit the user typed.
//!
//! After a search, [`Searcher::highlight`] (or
//! [`Searcher::highlight_text`] for [`Searcher::search_text`] queries)
//! re-runs the alignment of the query with one hit's term and walks it back.
//! It returns the parts of the caller's original term string that the
//! alignment matched, as sorted, disjoint, half-open ranges in code points,
//! UTF-8 bytes and UTF-16 units. Only call it for the hits you show: the
//! search itself does no highlighting work.
//!
//! A term character is highlighted when it is aligned with an equal query
//! character or is one of a transposed pair; substituted characters and
//! characters the user skipped are not. The full rules (normalisation,
//! partially matched characters such as `ß`, tie-breaking, prefix mode) are
//! in `docs/design/highlighting.md`.
//!
//! ```
//! use keyhammer::cost::CostModel;
//! use keyhammer::highlight::HighlightMode;
//! use keyhammer::search::{SearchConfig, Searcher};
//! use keyhammer::text::Normalizer;
//! use keyhammer::trie::Trie;
//!
//! let items = [("Coração", 10), ("Corrida", 5)];
//! let trie = Trie::build_normalized(&items, &Normalizer::new()).unwrap();
//! let cm = CostModel::qwerty();
//! let mut s = Searcher::new();
//! let q = "coracap"; // "p" for "o", a neighbouring key
//! let out = s.search_text(&trie, &cm, q, &SearchConfig::default()).unwrap();
//! let hit = &out.hits[0];
//! // The string the caller inserted for this term.
//! let source = items[trie.input_index(hit.id) as usize].0;
//! let h = s.highlight_text(&trie, &cm, q, hit, source, HighlightMode::Whole).unwrap();
//! assert_eq!(h.cost, hit.cost);
//! // "Coraçã" is highlighted, the final "o" was mistyped.
//! assert_eq!(h.ranges.len(), 1);
//! assert_eq!(h.ranges[0].chars, 0..6);
//! assert_eq!(h.ranges[0].utf8, 0..8); // ç and ã are two bytes each
//! assert_eq!(h.ranges[0].utf16, 0..6);
//! assert_eq!(&source[h.ranges[0].utf8.clone()], "Coraçã");
//! ```
//!
//! [`Searcher::highlight`]: crate::search::Searcher::highlight
//! [`Searcher::highlight_text`]: crate::search::Searcher::highlight_text
//! [`Searcher::search_text`]: crate::search::Searcher::search_text

use alloc::vec::Vec;
use core::fmt;
use core::ops::Range;

use crate::cost::{Cost, CostModel, INF};
use crate::search::{DELETE, Hit, INSERT, MAX_W, SUB, TRANSPOSE, moves};
use crate::text::{Normalizer, SourceMap};
use crate::trie::Trie;

/// Which search produced the hit.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HighlightMode {
    /// A hit of [`Searcher::search`](crate::search::Searcher::search) or
    /// [`Searcher::search_text`](crate::search::Searcher::search_text): the
    /// whole term is aligned with the query.
    #[default]
    Whole,
    /// A hit of [`Searcher::search_prefix`](crate::search::Searcher::search_prefix)
    /// or [`Searcher::search_prefix_text`](crate::search::Searcher::search_prefix_text):
    /// the query is aligned with the shortest prefix of the term that attains
    /// the hit's cost, and the rest of the term is never highlighted.
    Prefix,
}

/// One span of the caller's term string, in three units.
///
/// All three ranges are half-open and describe the same characters.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct MatchRange {
    /// Code points (`str::chars` positions; Python `str` indices).
    pub chars: Range<usize>,
    /// UTF-8 bytes: always on `char` boundaries, so `&source[r.utf8]` works.
    pub utf8: Range<usize>,
    /// UTF-16 code units (JavaScript string indices); never splits a
    /// surrogate pair.
    pub utf16: Range<usize>,
}

/// The highlighting of one hit.
#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Highlight {
    /// The highlighted spans of the source string: sorted, disjoint, never
    /// empty and never touching (at least one character between two spans).
    pub ranges: Vec<MatchRange>,
    /// The span of the source string the query was aligned with: the whole
    /// term in [`HighlightMode::Whole`], the matched prefix in
    /// [`HighlightMode::Prefix`]. Every range lies within it.
    pub aligned: MatchRange,
    /// The cost of the alignment behind the ranges: always equal to the
    /// hit's [`Hit::cost`].
    pub cost: Cost,
    /// Dynamic-programming cells computed (the deterministic work counter).
    pub cells: usize,
}

/// Why a hit could not be highlighted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum HighlightError {
    /// The query is longer than [`MAX_QUERY_LEN`](crate::search::MAX_QUERY_LEN).
    QueryTooLong {
        /// Query length in code points (after normalisation for
        /// `highlight_text`).
        len: usize,
        /// The limit.
        max: usize,
    },
    /// The hit's id is not a term of this trie.
    UnknownTerm {
        /// The id.
        id: u32,
    },
    /// The source string does not normalise (with the trie's normaliser, if
    /// any) to the hit's term.
    SourceMismatch,
    /// The alignment of this query and term does not cost what the hit says:
    /// the hit comes from another query, cost model, trie or mode.
    CostMismatch {
        /// The hit's cost.
        expected: Cost,
        /// The recomputed cost, or `None` when it was not computed because it
        /// certainly exceeds `expected` (or `expected` exceeds any budget a
        /// search accepts).
        found: Option<Cost>,
    },
}

impl fmt::Display for HighlightError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HighlightError::QueryTooLong { len, max } => {
                write!(f, "query has {len} code points, the limit is {max}")
            }
            HighlightError::UnknownTerm { id } => write!(f, "no term with id {id}"),
            HighlightError::SourceMismatch => {
                f.write_str("the source string does not normalise to the hit's term")
            }
            HighlightError::CostMismatch {
                expected,
                found: Some(found),
            } => write!(
                f,
                "the alignment costs {found}, the hit says {expected}: not a hit of this query"
            ),
            HighlightError::CostMismatch {
                expected,
                found: None,
            } => write!(
                f,
                "no alignment can cost {expected}: not a hit of this query"
            ),
        }
    }
}

/// What the traceback decided for one normalised term symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    /// After the aligned prefix (prefix mode).
    Unaligned,
    /// Aligned with an equal query symbol.
    Matched,
    /// Aligned with a different query symbol.
    Substituted,
    /// One of a transposed pair.
    Transposed,
    /// Skipped by the user (an insertion).
    Skipped,
}

impl Status {
    fn highlighted(self) -> bool {
        matches!(self, Status::Matched | Status::Transposed)
    }
}

/// Reusable buffers of the traceback, kept in the `Searcher`.
#[derive(Default)]
pub(crate) struct Scratch {
    /// The DP matrix, row `j` (term prefix length) after row `j - 1`.
    d: Vec<Cost>,
    /// The term symbols.
    t: Vec<u32>,
    /// One status per term symbol.
    pub(crate) status: Vec<Status>,
    /// The walked path, end first: (move, i, j) of each cell left.
    pub(crate) path: Vec<(usize, usize, usize)>,
    map: SourceMap,
}

/// Computes the matrix, checks the cost and walks the path back; fills
/// `s.status` and `s.path`. Returns the aligned prefix length and the cells
/// computed.
pub(crate) fn align(
    s: &mut Scratch,
    cm: &CostModel,
    q: &[u32],
    mode: HighlightMode,
    expected: Cost,
) -> Result<(usize, usize), HighlightError> {
    let (m, n) = (q.len(), s.t.len());
    let mismatch = HighlightError::CostMismatch {
        expected,
        found: None,
    };
    // A hit above the largest budget cannot come from a search, and a prefix
    // longer than `m + reach` needs more than `expected` in insertions.
    if expected > MAX_W as Cost * cm.c_indel_min() {
        return Err(mismatch);
    }
    let reach = usize::from(expected / cm.c_indel_min());
    let jmax = match mode {
        HighlightMode::Prefix => n.min(m + reach),
        HighlightMode::Whole if n > m + reach || m > n + reach => return Err(mismatch),
        HighlightMode::Whole => n,
    };
    let w = m + 1;
    let cells = (jmax + 1) * w;
    s.d.clear();
    s.d.resize(cells, INF);
    let (d, t) = (&mut s.d, &s.t);
    let cell = |d: &[Cost], j: usize, i: usize| d[j * w + i];
    // The candidates of cell (i, j), from the same function as the search.
    let cand = |d: &[Cost], j: usize, i: usize| {
        let ch = if j >= 1 { t[j - 1] } else { 0 };
        let parent = if j >= 2 { Some(t[j - 2]) } else { None };
        let diag = if j >= 1 && i >= 1 {
            cell(d, j - 1, i - 1)
        } else {
            INF
        };
        let up = if j >= 1 { cell(d, j - 1, i) } else { INF };
        let left = if i >= 1 { cell(d, j, i - 1) } else { INF };
        let skip2 = if j >= 2 && i >= 2 {
            cell(d, j - 2, i - 2)
        } else {
            INF
        };
        moves(cm, q, i, ch, parent, diag, up, left, skip2)
    };
    d[0] = 0;
    for j in 0..=jmax {
        for i in 0..=m {
            if i + j > 0 {
                let c = cand(d, j, i);
                d[j * w + i] = c[SUB].min(c[TRANSPOSE]).min(c[INSERT]).min(c[DELETE]);
            }
        }
    }
    // Whole: the full term. Prefix: the shortest prefix of minimal cost.
    let end = match mode {
        HighlightMode::Prefix => (0..=jmax).min_by_key(|&j| (cell(d, j, m), j)).unwrap_or(0),
        HighlightMode::Whole => n,
    };
    let cost = cell(d, end, m);
    if cost != expected {
        return Err(HighlightError::CostMismatch {
            expected,
            found: (cost < INF).then_some(cost),
        });
    }
    s.status.clear();
    s.status.resize(n, Status::Unaligned);
    s.path.clear();
    let (mut j, mut i) = (end, m);
    while i + j > 0 {
        let v = cell(d, j, i);
        let c = cand(d, j, i);
        // Tie-breaking: the first move, in this order, that attains the cell.
        let Some(mv) = [SUB, TRANSPOSE, INSERT, DELETE]
            .into_iter()
            .find(|&k| c[k] == v)
        else {
            // Unreachable: every finite cell is attained by one of its moves.
            break;
        };
        s.path.push((mv, i, j));
        match mv {
            SUB => {
                s.status[j - 1] = if q[i - 1] == t[j - 1] {
                    Status::Matched
                } else {
                    Status::Substituted
                };
                (i, j) = (i - 1, j - 1);
            }
            TRANSPOSE => {
                s.status[j - 1] = Status::Transposed;
                s.status[j - 2] = Status::Transposed;
                (i, j) = (i - 2, j - 2);
            }
            INSERT => {
                s.status[j - 1] = Status::Skipped;
                j -= 1;
            }
            _ => i -= 1,
        }
    }
    Ok((end, cells))
}

/// Highlights `hit` for the query symbols `q`; the entry point of
/// `Searcher::highlight` and `Searcher::highlight_text`.
pub(crate) fn run(
    s: &mut Scratch,
    trie: &Trie,
    cm: &CostModel,
    q: &[u32],
    hit: &Hit,
    source: &str,
    mode: HighlightMode,
) -> Result<Highlight, HighlightError> {
    if hit.id as usize >= trie.len() {
        return Err(HighlightError::UnknownTerm { id: hit.id });
    }
    // Without a normaliser the term is stored as given: the identity map.
    let n = trie.normalizer().unwrap_or(
        Normalizer::new()
            .with_case_folding(false)
            .with_diacritic_folding(false),
    );
    let term = n.normalize_mapped(source, &mut s.map);
    if term != trie.term(hit.id) {
        return Err(HighlightError::SourceMismatch);
    }
    s.t.clear();
    s.t.extend(term.chars().map(u32::from));
    let (end, cells) = align(s, cm, q, mode, hit.cost)?;
    let (ranges, aligned) = source_ranges(source, &s.map, &s.status, end);
    Ok(Highlight {
        ranges,
        aligned,
        cost: hit.cost,
        cells,
    })
}

/// The highlighted spans of `source`, and the span of its first `end`
/// normalised symbols, from the status of each normalised symbol.
///
/// A source character that produced symbols is highlighted when at least one
/// of them is, and is aligned when at least one of them is among the first
/// `end`; one that produced none (a dropped combining mark) takes both states
/// from the character before it (none at the start of the string).
fn source_ranges(
    source: &str,
    map: &SourceMap,
    status: &[Status],
    end: usize,
) -> (Vec<MatchRange>, MatchRange) {
    type Pos = (usize, usize, usize);
    let span = |a: Pos, b: Pos| MatchRange {
        chars: a.0..b.0,
        utf8: a.1..b.1,
        utf16: a.2..b.2,
    };
    // (chars, utf8, utf16) at the current position.
    let mut at: Pos = (0, 0, 0);
    let mut ranges = Vec::new();
    let mut open: Option<Pos> = None;
    let mut aligned: Option<(Pos, Pos)> = None;
    // Where an empty aligned span sits: the first character with a symbol.
    let mut first: Option<Pos> = None;
    let mut p = 0;
    let (mut lit, mut inside) = (false, false);
    for (k, ch) in source.chars().enumerate() {
        let p0 = p;
        let mut any = false;
        while map.source_of(p) == Some(k) {
            any |= status.get(p).is_some_and(|s| s.highlighted());
            p += 1;
        }
        if p > p0 {
            lit = any;
            inside = p0 < end;
            first.get_or_insert(at);
        }
        let next = (at.0 + 1, at.1 + ch.len_utf8(), at.2 + ch.len_utf16());
        if inside {
            aligned = Some((aligned.map_or(at, |a| a.0), next));
        }
        match (lit, open) {
            (true, None) => open = Some(at),
            (false, Some(o)) => {
                ranges.push(span(o, at));
                open = None;
            }
            _ => {}
        }
        at = next;
    }
    if let Some(o) = open {
        ranges.push(span(o, at));
    }
    let aligned = match aligned {
        Some((a, b)) => span(a, b),
        None => {
            let e = first.unwrap_or(at);
            span(e, e)
        }
    };
    (ranges, aligned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost::Layout;
    use crate::search::{SearchConfig, Searcher};
    use alloc::string::String;
    use alloc::vec;

    /// Small deterministic xorshift generator.
    struct Rng(u64);

    impl Rng {
        fn new(seed: u64) -> Self {
            Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
        }
        fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n as u64) as usize
        }
    }

    /// Naive full-matrix weighted OSA, written independently of `moves`:
    /// `d[j][i]` aligns `t[..j]` with `q[..i]`.
    fn oracle(cm: &CostModel, q: &[u32], t: &[u32]) -> Vec<Vec<u32>> {
        const BIG: u32 = 1_000_000;
        let (m, n) = (q.len(), t.len());
        let mut d = vec![vec![BIG; m + 1]; n + 1];
        d[0][0] = 0;
        for j in 0..=n {
            for i in 0..=m {
                let mut best = d[j][i];
                if i >= 1 && j >= 1 {
                    best = best
                        .min(d[j - 1][i - 1] + u32::from(cm.sub_cost(q[i - 1], t[j - 1], i - 1)));
                }
                if j >= 1 {
                    let prev_t = if j >= 2 { Some(t[j - 2]) } else { None };
                    best = best.min(d[j - 1][i] + u32::from(cm.ins_cost(t[j - 1], prev_t, i)));
                }
                if i >= 1 {
                    best = best.min(d[j][i - 1] + u32::from(cm.del_cost(q, i - 1)));
                }
                if i >= 2
                    && j >= 2
                    && q[i - 1] == t[j - 2]
                    && q[i - 2] == t[j - 1]
                    && q[i - 1] != q[i - 2]
                {
                    best = best.min(d[j - 2][i - 2] + u32::from(cm.transpose_cost(i - 2)));
                }
                d[j][i] = best;
            }
        }
        d
    }

    /// The value of each move into `d[j][i]`, in the documented tie order
    /// (diagonal, transposition, insertion, deletion), computed here from the
    /// cost model (not through `moves`); `None` if the move is not available.
    fn candidates(
        cm: &CostModel,
        q: &[u32],
        t: &[u32],
        d: &[Vec<u32>],
        i: usize,
        j: usize,
    ) -> [Option<u32>; 4] {
        let mut c = [None; 4];
        if i >= 1 && j >= 1 {
            c[0] = Some(d[j - 1][i - 1] + u32::from(cm.sub_cost(q[i - 1], t[j - 1], i - 1)));
        }
        if i >= 2 && j >= 2 && q[i - 1] == t[j - 2] && q[i - 2] == t[j - 1] && q[i - 1] != q[i - 2]
        {
            c[1] = Some(d[j - 2][i - 2] + u32::from(cm.transpose_cost(i - 2)));
        }
        if j >= 1 {
            let prev = if j >= 2 { Some(t[j - 2]) } else { None };
            c[2] = Some(d[j - 1][i] + u32::from(cm.ins_cost(t[j - 1], prev, i)));
        }
        if i >= 1 {
            c[3] = Some(d[j][i - 1] + u32::from(cm.del_cost(q, i - 1)));
        }
        c
    }

    /// Checks the walked path against the independent matrix `d`: it is a
    /// connected walk from its end cell to (0, 0), each step is the first
    /// move in the documented tie order that attains the cell's value, and
    /// the step costs sum to the value of the end cell, which is returned.
    fn check_path(
        cm: &CostModel,
        q: &[u32],
        t: &[u32],
        d: &[Vec<u32>],
        path: &[(usize, usize, usize)],
    ) -> u32 {
        let mut total = 0u32;
        let mut expect_at: Option<(usize, usize)> = None;
        for &(mv, i, j) in path {
            if let Some(at) = expect_at {
                assert_eq!(at, (i, j), "path not connected");
            }
            let c = candidates(cm, q, t, d, i, j);
            let order = [SUB, TRANSPOSE, INSERT, DELETE];
            let first = c.iter().position(|&v| v == Some(d[j][i]));
            assert_eq!(first.map(|k| order[k]), Some(mv), "tie rule at ({i}, {j})");
            let (prev, next) = match mv {
                SUB => (d[j - 1][i - 1], (i - 1, j - 1)),
                TRANSPOSE => (d[j - 2][i - 2], (i - 2, j - 2)),
                INSERT => (d[j - 1][i], (i, j - 1)),
                _ => (d[j][i - 1], (i - 1, j)),
            };
            total += d[j][i] - prev;
            expect_at = Some(next);
        }
        assert_eq!(
            expect_at.unwrap_or((0, 0)),
            (0, 0),
            "path does not reach (0, 0)"
        );
        total
    }

    fn syms(s: &str) -> Vec<u32> {
        s.chars().map(u32::from).collect()
    }

    const ALPHABETS: [&str; 6] = ["aqw", "asdfqwer", "abcdefghij", "aeéèçß", "жзaЖ😀", "çlp.;"];

    /// For random dictionaries and queries, every hit of both modes is
    /// highlighted; the path is legal, follows the tie rule, and its step
    /// costs sum to the hit's cost, which is the oracle cost; the statuses
    /// agree with the path.
    #[test]
    fn traceback_cost_equals_oracle_and_hit_cost() {
        let rounds = if cfg!(miri) { 3 } else { 120 };
        let (mut hits, mut prefix_hits, mut transposed, mut skipped) = (0, 0, 0, 0);
        for (a, alpha) in ALPHABETS.iter().enumerate() {
            let alpha = syms(alpha);
            for d in 0..rounds {
                let mut rng = Rng::new((a as u64 + 1) * 7919 + d);
                let cm = CostModel::for_layout(Layout::ALL[d as usize % Layout::ALL.len()]);
                let words: Vec<String> = (0..1 + rng.below(12))
                    .map(|_| {
                        (0..1 + rng.below(7))
                            .map(|_| char::from_u32(alpha[rng.below(alpha.len())]).unwrap_or('a'))
                            .collect()
                    })
                    .collect();
                let items: Vec<(&str, u16)> = words.iter().map(|w| (w.as_str(), 1)).collect();
                let Ok(trie) = Trie::build(&items) else {
                    continue;
                };
                let mut s = Searcher::new();
                for _ in 0..6 {
                    // A dictionary word with swaps and edits, or random.
                    let mut q = syms(&words[rng.below(words.len())]);
                    for _ in 0..rng.below(3) {
                        match rng.below(3) {
                            0 if q.len() > 1 => {
                                let i = rng.below(q.len() - 1);
                                q.swap(i, i + 1);
                            }
                            1 => q.insert(rng.below(q.len() + 1), alpha[rng.below(alpha.len())]),
                            _ if !q.is_empty() => {
                                q.remove(rng.below(q.len()));
                            }
                            _ => {}
                        }
                    }
                    let qs: String = q.iter().filter_map(|&c| char::from_u32(c)).collect();
                    let cfg = SearchConfig {
                        k: 50,
                        budget: 64,
                        ..SearchConfig::default()
                    };
                    for mode in [HighlightMode::Whole, HighlightMode::Prefix] {
                        let out = match mode {
                            HighlightMode::Whole => s.search(&trie, &cm, qs.as_bytes(), &cfg),
                            _ => s.search_prefix(&trie, &cm, qs.as_bytes(), &cfg),
                        };
                        for hit in out.unwrap_or_else(|e| panic!("{e}")).hits {
                            let term = trie.term(hit.id);
                            let t = syms(term);
                            let h = s
                                .highlight(&trie, &cm, qs.as_bytes(), &hit, term, mode)
                                .unwrap_or_else(|e| panic!("{e} q={qs:?} t={term:?} {mode:?}"));
                            let d = oracle(&cm, &q, &t);
                            let col: Vec<u32> = d.iter().map(|r| r[q.len()]).collect();
                            let want = match mode {
                                HighlightMode::Whole => col[t.len()],
                                _ => col.iter().copied().min().unwrap_or(u32::MAX),
                            };
                            assert_eq!(u32::from(h.cost), want);
                            assert_eq!(h.cost, hit.cost);
                            let sc = &s.hl;
                            assert_eq!(
                                check_path(&cm, &q, &t, &d, &sc.path),
                                want,
                                "q={qs:?} t={term:?}"
                            );
                            // Statuses follow the path.
                            for &(mv, i, j) in &sc.path {
                                match mv {
                                    SUB if q[i - 1] == t[j - 1] => {
                                        assert_eq!(sc.status[j - 1], Status::Matched)
                                    }
                                    SUB => assert_eq!(sc.status[j - 1], Status::Substituted),
                                    TRANSPOSE => {
                                        transposed += 1;
                                        assert_eq!(sc.status[j - 1], Status::Transposed);
                                        assert_eq!(sc.status[j - 2], Status::Transposed);
                                    }
                                    INSERT => {
                                        skipped += 1;
                                        assert_eq!(sc.status[j - 1], Status::Skipped)
                                    }
                                    _ => {}
                                }
                            }
                            if mode == HighlightMode::Prefix {
                                prefix_hits += 1;
                                // The shortest prefix of minimal cost.
                                let end = h.aligned.chars.end;
                                assert_eq!(col.iter().position(|&c| c == want), Some(end));
                                assert!(sc.status[end..].iter().all(|&x| x == Status::Unaligned));
                            } else {
                                hits += 1;
                                assert!(sc.status.iter().all(|&x| x != Status::Unaligned));
                            }
                        }
                    }
                }
            }
        }
        std::eprintln!(
            "{hits} whole-term hits, {prefix_hits} prefix hits, {transposed} transpositions, \
             {skipped} skipped symbols"
        );
        if !cfg!(miri) {
            assert!(hits > 2_000 && prefix_hits > 2_000 && transposed > 50 && skipped > 500);
        }
    }

    /// Among equal-cost alignments, the diagonal move wins at every cell, so
    /// edits go to the left.
    #[test]
    fn ties_put_edits_leftmost() {
        let cm = CostModel::qwerty();
        // "abab" against "ab": the two extra query symbols are dropped from
        // the middle, and both term symbols are matched with the last ones.
        let mut s = Scratch {
            t: syms("ab"),
            ..Scratch::default()
        };
        let q = syms("abab");
        let (end, _) = align(&mut s, &cm, &q, HighlightMode::Whole, 32).unwrap_or((9, 9));
        assert_eq!(end, 2);
        assert_eq!(s.status, [Status::Matched, Status::Matched]);
        let path: Vec<(usize, usize, usize)> = s.path.clone();
        assert_eq!(
            path,
            [(SUB, 4, 2), (DELETE, 3, 1), (DELETE, 2, 1), (SUB, 1, 1)]
        );
        // "abcab" against "ab": dropping "cab" or "bca" both cost 48 (dropping
        // "abc" costs 56, the first symbol weighs x1.5). The rule keeps the
        // last "b" and drops "bca".
        let q = syms("abcab");
        let (_, _) = align(&mut s, &cm, &q, HighlightMode::Whole, 48).unwrap_or((9, 9));
        assert_eq!(s.status, [Status::Matched, Status::Matched]);
        assert_eq!(s.path.first(), Some(&(SUB, 5, 2)));
        assert_eq!(s.path.last(), Some(&(SUB, 1, 1)));
    }

    #[test]
    fn impossible_costs_are_refused_before_any_work() {
        let cm = CostModel::qwerty();
        let mut s = Scratch {
            t: syms("abcdefghijklmnop"),
            ..Scratch::default()
        };
        let q = syms("a");
        for mode in [HighlightMode::Whole, HighlightMode::Prefix] {
            assert_eq!(
                align(&mut s, &cm, &q, mode, 65),
                Err(HighlightError::CostMismatch {
                    expected: 65,
                    found: None
                })
            );
        }
        // Fifteen insertions cannot cost 64.
        assert_eq!(
            align(&mut s, &cm, &q, HighlightMode::Whole, 64),
            Err(HighlightError::CostMismatch {
                expected: 64,
                found: None
            })
        );
        // In prefix mode the matrix stops at m + cost / 8 term symbols: two
        // rows of two cells for cost 0, ten rows for cost 64.
        assert_eq!(
            align(&mut s, &cm, &q, HighlightMode::Prefix, 0),
            Ok((1, 2 * 2))
        );
        assert_eq!(
            align(&mut s, &cm, &q, HighlightMode::Prefix, 64),
            Err(HighlightError::CostMismatch {
                expected: 64,
                found: Some(0)
            })
        );
    }
}

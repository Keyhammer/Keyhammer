// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! Exact best-first top-k search over a [`Trie`] with weighted edit costs.
//!
//! Each queued trie node carries one banded row of the edit-distance matrix.
//! A row cell `k` at trie depth `j` stands for query prefix length
//! `i = j + k - W`, where `W = budget / c_indel_min` is the band half-width.
//! A node is queued with a lower bound on the cost of every term below it, so
//! the first terminal popped is always the best remaining one.
//!
//! The budget, the band and the candidates always use the weighted costs of
//! [`CostModel`]. The order of the results is set by [`Ranking`]: by default
//! ([`Ranking::Coarse`]) terms are ranked by the weighted cost rounded up to
//! whole units of 16 ([`whole_units`]), then by higher weight, then by lower
//! term id, so the weight decides between terms whose costs round to the same
//! number of units. This is not a count of edits: the x1.5 factor on the
//! first byte makes an ordinary (non-neighbouring-key) edit there cost 24,
//! i.e. two units, while a neighbouring-key substitution there costs 12 (one
//! unit) and a transposition 18 (two units); two cheap edits (8 + 8) count as
//! one.
//! [`Ranking::Exact`] ranks by the exact weighted cost instead, then weight,
//! then id. In both modes [`Hit::cost`] is the exact weighted cost.
//!
//! [`Searcher::search_prefix`] is the autocomplete variant: a term costs what its best prefix
//! costs. See `docs/design/prefix-mode.md`.
//!
//! Queries and terms are compared per Unicode scalar value (a symbol), not per
//! UTF-8 byte; for ASCII the two are the same. See `docs/design/unicode.md`.

use alloc::collections::BinaryHeap;
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;
use core::fmt;

use crate::cost::{Cost, CostModel, INF, symbol_class, whole_units};
use crate::highlight::{self, Highlight, HighlightError, HighlightMode};
use crate::text;
use crate::trie::{NO_TERM, Trie};

/// Longest accepted query, in symbols (code points; bytes for ASCII).
pub const MAX_QUERY_LEN: usize = 128;
/// Largest band half-width: the budget is at most `MAX_W * c_indel_min`.
pub(crate) const MAX_W: usize = 8;
const ROW: usize = 2 * MAX_W + 1;

/// How results are ordered.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ranking {
    /// By the weighted cost rounded up to whole units of 16
    /// ([`whole_units`]), then higher weight, then lower term id. Terms whose
    /// costs round to the same number of units are ordered by weight. This is
    /// not a count of edits: the x1.5 factor on the first byte makes an
    /// ordinary (non-neighbouring-key) edit there cost 24, i.e. two units,
    /// while a neighbouring-key substitution there costs 12 (one unit) and a
    /// transposition 18 (two units); two cheap edits (8 + 8) count as one.
    #[default]
    Coarse,
    /// By the exact weighted cost, then higher weight, then lower term id.
    Exact,
}

impl Ranking {
    /// The cost used for ordering.
    #[inline]
    fn rank(self, cost: Cost) -> Cost {
        match self {
            Ranking::Coarse => whole_units(cost),
            Ranking::Exact => cost,
        }
    }
}

/// Search parameters.
///
/// Build it with `..SearchConfig::default()` so that new fields keep their
/// defaults: `k = 10`, `budget = 32` (two ordinary edits' worth of cost), `tsb = true`,
/// `max_nodes = 100_000`, `ranking = Ranking::Coarse`.
#[derive(Clone, Debug)]
pub struct SearchConfig {
    /// Number of results wanted.
    pub k: usize,
    /// Largest accepted edit cost (fixed point, 16 = one edit).
    pub budget: Cost,
    /// Use the subtree-signature lower bound (on by default).
    ///
    /// It never changes the results of a search that finishes within `max_nodes`, only how much work is done; if the node limit stops a search, both modes return a correct prefix of the same ranked list, but its length can differ. In
    /// `docs/benchmarks/tsb-default.md` it expanded about a third fewer trie
    /// nodes and lowered p95 latency by about a fifth (one machine, one
    /// corpus). The per-node data it reads is built by [`Trie::build`]
    /// whatever this flag says, so turning it off saves no memory; set
    /// `tsb: false` to compare or to measure the plain bound.
    pub tsb: bool,
    /// Hard limit on expanded trie nodes.
    pub max_nodes: usize,
    /// How results are ordered.
    pub ranking: Ranking,
}

impl SearchConfig {
    /// An opt-in configuration that trades speed for recall: the default
    /// settings with `budget = 48` (three ordinary edits' worth of cost). It
    /// sets the subtree bound on explicitly (`tsb = true`, which does not change
    /// the results and is also the default).
    ///
    /// On the benchmark in `docs/benchmarks/recall-preset.md` (300 Birkbeck
    /// typo pairs, one machine, median of three runs) it found the right word more often
    /// than the default at every dictionary size, at the price of expanding
    /// several times more trie nodes per query and a higher latency; the
    /// measured numbers are in that file. Measure it on your own dictionary
    /// before relying on it. The maximum accepted budget is 64 (see
    /// [`SearchError::BudgetTooLarge`]); 64 is not offered as a preset because
    /// its measured cost grows much faster than its gain.
    ///
    /// ```
    /// use keyhammer::search::SearchConfig;
    /// let cfg = SearchConfig { k: 5, ..SearchConfig::high_recall() };
    /// assert_eq!(cfg.budget, 48);
    /// assert!(cfg.tsb);
    /// ```
    #[must_use]
    pub fn high_recall() -> Self {
        Self {
            budget: 48,
            tsb: true,
            ..Self::default()
        }
    }
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            k: 10,
            budget: 32,
            tsb: true,
            max_nodes: 100_000,
            ranking: Ranking::Coarse,
        }
    }
}

/// One result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    /// Term id (see [`Trie::term`]): the term's position in the byte-sorted,
    /// deduplicated list, not in the slice given to [`Trie::build`];
    /// [`Trie::input_index`] maps it back.
    pub id: u32,
    /// Exact weighted edit cost between the query and the term, whatever the
    /// [`Ranking`].
    pub cost: Cost,
    /// The term's weight.
    pub weight: u16,
}

/// Work counters for one search.
///
/// Non-exhaustive: fields may be added, so build it with `Stats::default()` if at all.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Stats {
    /// Trie nodes expanded.
    pub nodes_expanded: usize,
    /// Queue entries created (nodes and terminals).
    pub nodes_pushed: usize,
    /// Banded DP rows computed (the root row plus one per child considered).
    /// The row-free expansions of a prefix search (see
    /// [`Searcher::search_prefix`]) count in `nodes_expanded` but not here.
    pub rows_computed: usize,
    /// The node limit stopped the search early.
    pub truncated: bool,
}

/// Hits in ranking order plus work counters.
#[derive(Clone, Debug)]
pub struct Output {
    /// Best first.
    pub hits: Vec<Hit>,
    /// Work counters.
    pub stats: Stats,
}

/// Why a search was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SearchError {
    /// The query is longer than [`MAX_QUERY_LEN`].
    QueryTooLong {
        /// Query length in code points (for [`Searcher::search_text`], after
        /// normalisation).
        len: usize,
        /// The limit.
        max: usize,
    },
    /// The budget needs a wider band than the implementation supports.
    BudgetTooLarge {
        /// The requested budget.
        budget: Cost,
        /// The largest supported budget for this cost model.
        max: Cost,
    },
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SearchError::QueryTooLong { len, max } => {
                write!(f, "query has {len} code points, the limit is {max}")
            }
            SearchError::BudgetTooLarge { budget, max } => {
                write!(f, "budget {budget} exceeds the maximum {max}")
            }
        }
    }
}

struct Entry {
    key: u64,
    seq: u32,
    node: u32,
    term: u32,
    /// Exact weighted cost of a terminal entry. For a node entry of a prefix
    /// search: the best cost of a prefix ending at or above the node (`INF` if
    /// none); unused by exact search.
    cost: Cost,
    /// Prefix search only: every term at or below this node costs exactly
    /// `cost`, so no row is needed to expand it.
    settled: bool,
    depth: u16,
    cur: [Cost; ROW],
    prev: [Cost; ROW],
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Entry {}
impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Entry {
    // Reversed so that `BinaryHeap` pops the smallest key first.
    fn cmp(&self, other: &Self) -> Ordering {
        (other.key, other.seq).cmp(&(self.key, self.seq))
    }
}

/// Queue key: ranking cost, then higher weight, then lower id.
///
/// `rank` is the ranking cost ([`Ranking::rank`]) of either a terminal's exact
/// cost or a node's lower bound. Both ranking functions are monotone
/// non-decreasing in the cost, so the rank of a node's lower bound never
/// exceeds the rank of any term below it, and with the subtree's largest
/// weight the node key stays a lower bound on every key below it: the bound
/// remains admissible and the first terminal popped is still the best one.
#[inline]
fn pack(rank: Cost, weight: u16, id: u32) -> u64 {
    (u64::from(rank) << 48) | (u64::from(u16::MAX - weight) << 32) | u64::from(id)
}

#[inline]
fn cap(v: Cost, budget: Cost) -> Cost {
    if v > budget { INF } else { v }
}

/// Index of the substitution (or match) in the array of [`moves`].
pub(crate) const SUB: usize = 0;
/// Index of the transposition in the array of [`moves`].
pub(crate) const TRANSPOSE: usize = 1;
/// Index of the insertion (a skipped term symbol) in the array of [`moves`].
pub(crate) const INSERT: usize = 2;
/// Index of the deletion (an extra query symbol) in the array of [`moves`].
pub(crate) const DELETE: usize = 3;

/// The four candidate values of the weighted OSA cell `D[i][j]` (query prefix
/// `q[..i]`, term prefix of length `j` ending in `ch`, whose previous symbol
/// is `parent`, `None` when `j <= 1`), indexed by [`SUB`], [`TRANSPOSE`],
/// [`INSERT`] and [`DELETE`]; `INF` for a move that is not available.
///
/// `diag`, `up`, `left` and `skip2` are `D[i-1][j-1]`, `D[i][j-1]`,
/// `D[i-1][j]` and `D[i-2][j-2]` (`INF` when absent). This is the only place
/// where the cost model's step costs enter a DP cell: the rows of the search
/// and the traceback of [`crate::highlight`] both use it, so highlighting
/// cannot drift from the costs the search reports.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
pub(crate) fn moves(
    cm: &CostModel,
    q: &[u32],
    i: usize,
    ch: u32,
    parent: Option<u32>,
    diag: Cost,
    up: Cost,
    left: Cost,
    skip2: Cost,
) -> [Cost; 4] {
    let mut out = [INF; 4];
    if i >= 1 && diag < INF {
        out[SUB] = diag + cm.sub_cost(q[i - 1], ch, i - 1);
    }
    if i >= 2 && parent == Some(q[i - 1]) && q[i - 2] == ch && q[i - 1] != q[i - 2] && skip2 < INF {
        out[TRANSPOSE] = skip2 + cm.transpose_cost(i - 2);
    }
    if up < INF {
        out[INSERT] = up + cm.ins_cost(ch, parent, i);
    }
    if i >= 1 && left < INF {
        out[DELETE] = left + cm.del_cost(q, i - 1);
    }
    out
}

/// The smallest of the four candidates of [`moves`].
#[inline(always)]
fn best(c: [Cost; 4]) -> Cost {
    c[SUB].min(c[TRANSPOSE]).min(c[INSERT]).min(c[DELETE])
}

fn root_row(q: &[u32], cm: &CostModel, w: usize, budget: Cost) -> [Cost; ROW] {
    let mut row = [INF; ROW];
    row[w] = 0;
    for i in 1..=q.len().min(w) {
        // Row 0 of the term: only deletions (no term symbol, so `ch` is unused).
        let c = moves(cm, q, i, 0, None, INF, INF, row[w + i - 1], INF);
        row[w + i] = cap(best(c), budget);
    }
    row
}

#[allow(clippy::too_many_arguments)]
fn child_row(
    q: &[u32],
    cm: &CostModel,
    w: usize,
    budget: Cost,
    depth: usize,
    parent_label: u32,
    ch: u32,
    cur: &[Cost; ROW],
    prev: &[Cost; ROW],
) -> [Cost; ROW] {
    let m = q.len();
    let j = depth + 1;
    let parent = if depth >= 1 { Some(parent_label) } else { None };
    let mut out = [INF; ROW];
    for k in 0..=2 * w {
        let i_signed = (j + k) as isize - w as isize;
        if i_signed < 0 || i_signed as usize > m {
            continue;
        }
        let i = i_signed as usize;
        // Cell k of this row is D[i][j]; cell k of `cur` is D[i-1][j-1],
        // k + 1 of `cur` is D[i][j-1], k - 1 of `out` is D[i-1][j] and k of
        // `prev` is D[i-2][j-2].
        let up = if k < 2 * w { cur[k + 1] } else { INF };
        let left = if k >= 1 { out[k - 1] } else { INF };
        let c = moves(cm, q, i, ch, parent, cur[k], up, left, prev[k]);
        out[k] = cap(best(c), budget);
    }
    out
}

/// Lower bound on the cost of every term at or below `node`.
///
/// Any alignment either crosses trie row `depth` (bounded by the cells of
/// `cur`) or skips it with a transposition from row `depth - 1` (bounded by
/// `prev` plus the cheapest transposition). With `tsb`, the cells of `cur` are
/// raised by what the query suffix still has to pay given the letters and the
/// lengths present below the node.
#[allow(clippy::too_many_arguments)]
fn lower_bound(
    trie: &Trie,
    node: usize,
    depth: usize,
    cur: &[Cost; ROW],
    prev: &[Cost; ROW],
    m: usize,
    w: usize,
    qmask: &[u64],
    cm: &CostModel,
    tsb: bool,
    prefix: bool,
) -> Cost {
    let mut lb = INF;
    for (k, &d) in cur.iter().enumerate().take(2 * w + 1) {
        if d >= INF {
            continue;
        }
        let mut extra: Cost = 0;
        if tsb {
            let i = depth + k - w;
            let r = m - i;
            let lo = usize::from(trie.len_min(node)) - depth;
            let hi = usize::from(trie.len_max(node)) - depth;
            // Exact mode: the whole term must be aligned, so its length is in
            // `lo..=hi`. Prefix mode: any prefix of the term, so only the upper
            // end constrains the length.
            let gap = if !prefix && r < lo {
                lo - r
            } else {
                r.saturating_sub(hi)
            };
            let comp = (gap.min(usize::from(INF)) as Cost).saturating_mul(cm.c_indel_min());
            let missing = (qmask[i] & !trie.below_mask(node)).count_ones() as Cost * cm.c_min();
            extra = comp.max(missing);
        }
        lb = lb.min(d.saturating_add(extra));
    }
    if depth >= 1 {
        for &p in prev.iter().take(2 * w + 1) {
            if p < INF {
                lb = lb.min(p.saturating_add(cm.c_transpose_min()));
            }
        }
    }
    lb
}

/// The per-query inputs of one node expansion. [`Searcher::search`] computes
/// every row, bound and terminal cost through it, and so do the unit tests
/// that check the bound against a brute-force oracle.
struct Expander<'a> {
    trie: &'a Trie,
    cm: &'a CostModel,
    q: &'a [u32],
    qmask: &'a [u64],
    w: usize,
    budget: Cost,
    tsb: bool,
    prefix: bool,
}

impl Expander<'_> {
    /// Prefix mode: the cost of aligning the whole query with the term prefix
    /// that ends at this node (depth `depth`, row `cur`), or `INF`.
    fn prefix_cell(&self, depth: usize, cur: &[Cost; ROW]) -> Cost {
        match (self.q.len() + self.w).checked_sub(depth) {
            Some(k) if k <= 2 * self.w => cur[k],
            _ => INF,
        }
    }

    /// The root row and the lower bound of the root.
    fn root(&self) -> ([Cost; ROW], Cost) {
        let cur = root_row(self.q, self.cm, self.w, self.budget);
        let lb = lower_bound(
            self.trie,
            0,
            0,
            &cur,
            &[INF; ROW],
            self.q.len(),
            self.w,
            self.qmask,
            self.cm,
            self.tsb,
            self.prefix,
        );
        (cur, lb)
    }

    /// The id and exact cost of the term that ends at `v` (at `depth`, with
    /// row `cur`), if there is one and its cost is within the budget.
    fn terminal(&self, v: usize, depth: usize, cur: &[Cost; ROW]) -> Option<(u32, Cost)> {
        let tid = self.trie.term_id(v);
        if tid == NO_TERM {
            return None;
        }
        let k = (self.q.len() + self.w).checked_sub(depth)?;
        if k <= 2 * self.w && cur[k] <= self.budget {
            Some((tid, cur[k]))
        } else {
            None
        }
    }

    /// The row and the lower bound of child `c` of node `v`, where `v` is at
    /// `depth` with row `cur` and its parent's row `prev`.
    fn child(
        &self,
        v: usize,
        depth: usize,
        c: usize,
        cur: &[Cost; ROW],
        prev: &[Cost; ROW],
    ) -> ([Cost; ROW], Cost) {
        let row = child_row(
            self.q,
            self.cm,
            self.w,
            self.budget,
            depth,
            self.trie.symbol(v),
            self.trie.symbol(c),
            cur,
            prev,
        );
        let lb = lower_bound(
            self.trie,
            c,
            depth + 1,
            &row,
            cur,
            self.q.len(),
            self.w,
            self.qmask,
            self.cm,
            self.tsb,
            self.prefix,
        );
        (row, lb)
    }
}

/// A reusable search context. It keeps its priority queue and scratch buffers
/// across queries; the returned [`Output`] is still allocated per query.
#[derive(Default)]
pub struct Searcher {
    heap: BinaryHeap<Entry>,
    qmask: Vec<u64>,
    /// The query as symbols (see `text::decode`).
    qsym: Vec<u32>,
    /// The normalised query of `search_text`.
    qtext: String,
    seq: u32,
    /// Buffers of the highlighting traceback.
    pub(crate) hl: highlight::Scratch,
}

impl Searcher {
    /// Creates an empty context.
    pub fn new() -> Self {
        Self::default()
    }

    // Seven arguments since terminal entries carry their exact cost; a
    // parameter struct would add more code than it removes, so the lint is
    // allowed here.
    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        key: u64,
        node: u32,
        term: u32,
        cost: Cost,
        depth: u16,
        cur: [Cost; ROW],
        prev: [Cost; ROW],
    ) {
        self.seq = self.seq.wrapping_add(1);
        self.heap.push(Entry {
            key,
            seq: self.seq,
            node,
            term,
            cost,
            settled: false,
            depth,
            cur,
            prev,
        });
    }

    fn push_settled(&mut self, key: u64, node: u32, term: u32, cost: Cost, depth: u16) {
        self.seq = self.seq.wrapping_add(1);
        self.heap.push(Entry {
            key,
            seq: self.seq,
            node,
            term,
            cost,
            settled: true,
            depth,
            cur: [INF; ROW],
            prev: [INF; ROW],
        });
    }

    /// Validates the query length (in symbols) and the budget; returns the
    /// band half-width.
    fn check(len: usize, cm: &CostModel, cfg: &SearchConfig) -> Result<usize, SearchError> {
        if len > MAX_QUERY_LEN {
            return Err(SearchError::QueryTooLong {
                len,
                max: MAX_QUERY_LEN,
            });
        }
        let max_budget = MAX_W as Cost * cm.c_indel_min();
        if cfg.budget > max_budget {
            return Err(SearchError::BudgetTooLarge {
                budget: cfg.budget,
                max: max_budget,
            });
        }
        Ok(usize::from(cfg.budget / cm.c_indel_min()))
    }

    /// Returns the `cfg.k` best terms within `cfg.budget`, best first, in the
    /// order set by `cfg.ranking`.
    ///
    /// `q` is UTF-8 and is compared per code point with the terms, as it is:
    /// it must already be in the form the terms are stored in (lowercase for
    /// a dictionary of lowercase terms). [`Searcher::search_text`] normalises
    /// it for a trie built with [`Trie::build_normalized`]. A byte that is not
    /// part of valid UTF-8 is a symbol of its own that matches no term.
    pub fn search(
        &mut self,
        trie: &Trie,
        cm: &CostModel,
        q: &[u8],
        cfg: &SearchConfig,
    ) -> Result<Output, SearchError> {
        let len = text::decode(q, &mut self.qsym, MAX_QUERY_LEN);
        self.run(trie, cm, len, cfg)
    }

    /// Fills the per-query masks from `q` and resets the queue.
    fn prepare(&mut self, q: &[u32]) {
        let m = q.len();
        self.qmask.clear();
        self.qmask.resize(m + 1, 0);
        for i in (0..m).rev() {
            self.qmask[i] = self.qmask[i + 1] | symbol_class(q[i]);
        }
        self.heap.clear();
        self.seq = 0;
    }

    /// Normalises `q` with the trie's normaliser, if it has one, and decodes
    /// it into `self.qsym`; returns its length in symbols.
    fn load_text(&mut self, trie: &Trie, q: &str) -> usize {
        match trie.normalizer() {
            Some(n) => {
                let mut t = core::mem::take(&mut self.qtext);
                n.normalize_into(q, &mut t);
                let len = text::decode(t.as_bytes(), &mut self.qsym, MAX_QUERY_LEN);
                self.qtext = t;
                len
            }
            None => text::decode(q.as_bytes(), &mut self.qsym, MAX_QUERY_LEN),
        }
    }

    /// [`Searcher::search`] for text: `q` is normalised with the normaliser
    /// of `trie` ([`Trie::build_normalized`]) first, so terms and queries are
    /// folded identically. For a trie built with [`Trie::build`] this is
    /// `search(trie, cm, q.as_bytes(), cfg)`. [`MAX_QUERY_LEN`] applies to the
    /// normalised query.
    ///
    /// ```
    /// use keyhammer::cost::CostModel;
    /// use keyhammer::search::{SearchConfig, Searcher};
    /// use keyhammer::text::Normalizer;
    /// use keyhammer::trie::Trie;
    ///
    /// let items = [("São Paulo", 10), ("Straße", 5), ("Crème brûlée", 7)];
    /// let trie = Trie::build_normalized(&items, &Normalizer::new()).unwrap();
    /// let mut s = Searcher::new();
    /// let cm = CostModel::qwerty();
    /// let cfg = SearchConfig::default();
    /// let first = |s: &mut Searcher, q: &str| {
    ///     let out = s.search_text(&trie, &cm, q, &cfg).unwrap();
    ///     (trie.input_index(out.hits[0].id), out.hits[0].cost)
    /// };
    /// assert_eq!(first(&mut s, "SAO PAULO"), (0, 0));
    /// assert_eq!(first(&mut s, "strasse"), (1, 0));
    /// assert_eq!(first(&mut s, "creme brulee"), (2, 0));
    /// assert_eq!(first(&mut s, "sao paolo"), (0, 16)); // one typo
    /// ```
    pub fn search_text(
        &mut self,
        trie: &Trie,
        cm: &CostModel,
        q: &str,
        cfg: &SearchConfig,
    ) -> Result<Output, SearchError> {
        let len = self.load_text(trie, q);
        self.run(trie, cm, len, cfg)
    }

    /// [`Searcher::search_prefix`] for text, with the query normalised as by
    /// [`Searcher::search_text`].
    ///
    /// ```
    /// use keyhammer::cost::CostModel;
    /// use keyhammer::search::{SearchConfig, Searcher};
    /// use keyhammer::text::Normalizer;
    /// use keyhammer::trie::Trie;
    ///
    /// let items = [("Ação", 10), ("Acapulco", 5)];
    /// let trie = Trie::build_normalized(&items, &Normalizer::new()).unwrap();
    /// let out = Searcher::new()
    ///     .search_prefix_text(&trie, &CostModel::qwerty(), "AÇÃ", &SearchConfig::default())
    ///     .unwrap();
    /// assert_eq!(trie.term(out.hits[0].id), "acao");
    /// assert_eq!(out.hits[0].cost, 0);
    /// ```
    pub fn search_prefix_text(
        &mut self,
        trie: &Trie,
        cm: &CostModel,
        q: &str,
        cfg: &SearchConfig,
    ) -> Result<Output, SearchError> {
        let len = self.load_text(trie, q);
        self.run_prefix(trie, cm, len, cfg)
    }

    /// The characters of a hit that the query matched, as ranges of `source`
    /// in code points, UTF-8 bytes and UTF-16 units (see
    /// [`crate::highlight`] and `docs/design/highlighting.md`).
    ///
    /// `q` is the query exactly as given to [`Searcher::search`]
    /// ([`HighlightMode::Whole`]) or [`Searcher::search_prefix`]
    /// ([`HighlightMode::Prefix`]), and `hit` one of the hits that search
    /// returned. `source` is the string the caller inserted for the term: for
    /// a trie from [`Trie::build_normalized`], the entry
    /// `items[trie.input_index(hit.id)]`; for a trie from [`Trie::build`],
    /// the term itself ([`Trie::term`]`(hit.id)`). It must normalise, with
    /// the trie's normaliser if it has one, to the hit's term.
    ///
    /// The alignment is recomputed on the full matrix and walked back; its
    /// cost always equals `hit.cost` ([`Highlight::cost`]). The search itself
    /// does no highlighting work: call this only for the hits you show. The
    /// work per hit is at most `(m + 1) * (m + 9)` matrix cells for a query
    /// of `m` symbols ([`Highlight::cells`]), plus a pass over `source`.
    ///
    /// # Errors
    ///
    /// [`HighlightError::QueryTooLong`] as for the search,
    /// [`HighlightError::UnknownTerm`] if `hit.id` is not a term of `trie`,
    /// [`HighlightError::SourceMismatch`] if `source` does not normalise to
    /// the term, [`HighlightError::CostMismatch`] if the alignment does not
    /// cost `hit.cost` (a hit of another query, cost model or mode).
    ///
    /// # Panics
    ///
    /// Never, for any input.
    ///
    /// # Examples
    ///
    /// ```
    /// use keyhammer::cost::CostModel;
    /// use keyhammer::highlight::HighlightMode;
    /// use keyhammer::search::{SearchConfig, Searcher};
    /// use keyhammer::trie::Trie;
    ///
    /// let trie = Trie::build(&[("javascript", 10), ("java", 30)]).unwrap();
    /// let cm = CostModel::qwerty();
    /// let mut s = Searcher::new();
    /// let cfg = SearchConfig::default();
    ///
    /// // "javasript" skips the "c": everything else is highlighted.
    /// let out = s.search(&trie, &cm, b"javasript", &cfg).unwrap();
    /// let hit = &out.hits[0];
    /// let term = trie.term(hit.id);
    /// let h = s
    ///     .highlight(&trie, &cm, b"javasript", hit, term, HighlightMode::Whole)
    ///     .unwrap();
    /// let spans: Vec<&str> = h.ranges.iter().map(|r| &term[r.utf8.clone()]).collect();
    /// assert_eq!(spans, ["javas", "ript"]);
    /// assert_eq!(h.cost, hit.cost);
    ///
    /// // Prefix mode: only the typed prefix is highlighted.
    /// let out = s.search_prefix(&trie, &cm, b"jav", &cfg).unwrap();
    /// for hit in &out.hits {
    ///     let term = trie.term(hit.id);
    ///     let h = s
    ///         .highlight(&trie, &cm, b"jav", hit, term, HighlightMode::Prefix)
    ///         .unwrap();
    ///     assert_eq!(h.ranges[0].chars, 0..3);
    ///     assert_eq!(h.aligned.chars, 0..3);
    /// }
    /// ```
    pub fn highlight(
        &mut self,
        trie: &Trie,
        cm: &CostModel,
        q: &[u8],
        hit: &Hit,
        source: &str,
        mode: HighlightMode,
    ) -> Result<Highlight, HighlightError> {
        let len = text::decode(q, &mut self.qsym, MAX_QUERY_LEN);
        self.run_highlight(trie, cm, len, hit, source, mode)
    }

    /// [`Searcher::highlight`] for a hit of [`Searcher::search_text`]
    /// ([`HighlightMode::Whole`]) or [`Searcher::search_prefix_text`]
    /// ([`HighlightMode::Prefix`]): `q` is normalised with the trie's
    /// normaliser first, as those searches do.
    ///
    /// # Errors
    ///
    /// As for [`Searcher::highlight`].
    ///
    /// # Panics
    ///
    /// Never, for any input.
    ///
    /// # Examples
    ///
    /// ```
    /// use keyhammer::cost::CostModel;
    /// use keyhammer::highlight::HighlightMode;
    /// use keyhammer::search::{SearchConfig, Searcher};
    /// use keyhammer::text::Normalizer;
    /// use keyhammer::trie::Trie;
    ///
    /// let items = [("Straße", 1)];
    /// let trie = Trie::build_normalized(&items, &Normalizer::new()).unwrap();
    /// let cm = CostModel::qwerty();
    /// let mut s = Searcher::new();
    /// let out = s.search_text(&trie, &cm, "STRASE", &SearchConfig::default()).unwrap();
    /// let hit = &out.hits[0];
    /// let h = s
    ///     .highlight_text(&trie, &cm, "STRASE", hit, items[0].0, HighlightMode::Whole)
    ///     .unwrap();
    /// // One of the two "s" of "ß" was typed: "ß" counts as typed.
    /// assert_eq!(h.ranges.len(), 1);
    /// assert_eq!(h.ranges[0].chars, 0..6);
    /// assert_eq!(h.ranges[0].utf8, 0..7);
    /// ```
    pub fn highlight_text(
        &mut self,
        trie: &Trie,
        cm: &CostModel,
        q: &str,
        hit: &Hit,
        source: &str,
        mode: HighlightMode,
    ) -> Result<Highlight, HighlightError> {
        let len = self.load_text(trie, q);
        self.run_highlight(trie, cm, len, hit, source, mode)
    }

    /// [`Searcher::highlight`] on the `len` symbols in `self.qsym`.
    fn run_highlight(
        &mut self,
        trie: &Trie,
        cm: &CostModel,
        len: usize,
        hit: &Hit,
        source: &str,
        mode: HighlightMode,
    ) -> Result<Highlight, HighlightError> {
        if len > MAX_QUERY_LEN {
            return Err(HighlightError::QueryTooLong {
                len,
                max: MAX_QUERY_LEN,
            });
        }
        highlight::run(&mut self.hl, trie, cm, &self.qsym, hit, source, mode)
    }

    /// [`Searcher::search`] on the `len` symbols in `self.qsym`.
    fn run(
        &mut self,
        trie: &Trie,
        cm: &CostModel,
        len: usize,
        cfg: &SearchConfig,
    ) -> Result<Output, SearchError> {
        let w = Self::check(len, cm, cfg)?;
        let mut out = Output {
            hits: Vec::new(),
            stats: Stats::default(),
        };
        if cfg.k == 0 {
            return Ok(out);
        }
        // Moved out for the duration of the search so that the expander can
        // borrow them while entries are pushed; put back below.
        let qsym = core::mem::take(&mut self.qsym);
        let q = qsym.as_slice();
        self.prepare(q);
        let qmask = core::mem::take(&mut self.qmask);
        let ex = Expander {
            trie,
            cm,
            q,
            qmask: &qmask,
            w,
            budget: cfg.budget,
            tsb: cfg.tsb,
            prefix: false,
        };
        let rank = |c: Cost| cfg.ranking.rank(c);
        let (root_cur, lb) = ex.root();
        out.stats.rows_computed = 1;
        if lb <= cfg.budget {
            self.push(
                pack(rank(lb), trie.max_weight(0), 0),
                0,
                NO_TERM,
                0,
                0,
                root_cur,
                [INF; ROW],
            );
        }

        while let Some(e) = self.heap.pop() {
            if e.term != NO_TERM {
                out.hits.push(Hit {
                    id: e.term,
                    cost: e.cost,
                    weight: trie.weight(e.term),
                });
                if out.hits.len() == cfg.k {
                    break;
                }
                continue;
            }
            if out.stats.nodes_expanded >= cfg.max_nodes {
                out.stats.truncated = true;
                break;
            }
            out.stats.nodes_expanded += 1;
            let v = e.node as usize;
            let depth = usize::from(e.depth);

            if let Some((tid, cost)) = ex.terminal(v, depth, &e.cur) {
                let key = pack(rank(cost), trie.weight(tid), tid);
                self.push(key, e.node, tid, cost, e.depth, [INF; ROW], [INF; ROW]);
            }

            for c in trie.children(v) {
                let (row, lb) = ex.child(v, depth, c, &e.cur, &e.prev);
                out.stats.rows_computed += 1;
                if lb <= cfg.budget {
                    self.push(
                        pack(rank(lb), trie.max_weight(c), 0),
                        c as u32,
                        NO_TERM,
                        0,
                        e.depth + 1,
                        row,
                        e.cur,
                    );
                }
            }
        }
        self.qmask = qmask;
        self.qsym = qsym;
        out.stats.nodes_pushed = self.seq as usize;
        Ok(out)
    }

    /// Prefix (autocomplete) search: returns the `cfg.k` best terms that
    /// *start with* something within `cfg.budget` of the query, best first.
    ///
    /// The cost of a term is the smallest weighted edit cost between the whole
    /// query and any prefix of the term (the empty prefix included), so the
    /// typed text may have typos while the rest of the term is free. The order
    /// is the one of [`Searcher::search`]: [`Ranking`] cost, then higher
    /// weight, then lower term id, and [`Hit::cost`] is that exact prefix cost.
    /// A hit is the full term; the length of the matched prefix is not
    /// reported. An empty query matches every term at cost 0, so the result is
    /// the `k` heaviest terms.
    ///
    /// Configuration fields keep their meaning: `budget` bounds the prefix
    /// cost, `tsb` selects the subtree bound (results never change with it),
    /// `max_nodes` limits expanded trie nodes, including nodes expanded
    /// without a row (see `docs/design/prefix-mode.md`); `Stats::truncated`
    /// tells if it stopped the search, and the hits returned are then still
    /// the first ones of the exact order.
    ///
    /// `q` is read as for [`Searcher::search`].
    ///
    /// ```
    /// use keyhammer::cost::CostModel;
    /// use keyhammer::search::{SearchConfig, Searcher};
    /// use keyhammer::trie::Trie;
    ///
    /// let trie = Trie::build(&[("javascript", 10), ("java", 30), ("python", 50)]).unwrap();
    /// let mut s = Searcher::new();
    /// // "javs" types "s" for "a" (neighbouring keys); "java" starts both terms.
    /// let out = s
    ///     .search_prefix(&trie, &CostModel::qwerty(), b"javs", &SearchConfig::default())
    ///     .unwrap();
    /// let terms: Vec<&str> = out.hits.iter().map(|h| trie.term(h.id)).collect();
    /// assert_eq!(terms, ["java", "javascript"]);
    /// ```
    pub fn search_prefix(
        &mut self,
        trie: &Trie,
        cm: &CostModel,
        q: &[u8],
        cfg: &SearchConfig,
    ) -> Result<Output, SearchError> {
        let len = text::decode(q, &mut self.qsym, MAX_QUERY_LEN);
        self.run_prefix(trie, cm, len, cfg)
    }

    /// [`Searcher::search_prefix`] on the `len` symbols in `self.qsym`.
    fn run_prefix(
        &mut self,
        trie: &Trie,
        cm: &CostModel,
        len: usize,
        cfg: &SearchConfig,
    ) -> Result<Output, SearchError> {
        let w = Self::check(len, cm, cfg)?;
        let mut out = Output {
            hits: Vec::new(),
            stats: Stats::default(),
        };
        if cfg.k == 0 {
            return Ok(out);
        }
        let qsym = core::mem::take(&mut self.qsym);
        let q = qsym.as_slice();
        self.prepare(q);
        let qmask = core::mem::take(&mut self.qmask);
        let ex = Expander {
            trie,
            cm,
            q,
            qmask: &qmask,
            w,
            budget: cfg.budget,
            tsb: cfg.tsb,
            prefix: true,
        };
        let rank = |c: Cost| cfg.ranking.rank(c);
        // `acc` is the best cost of a prefix ending at or above a node, `lbd`
        // the bound on the prefixes strictly below it; every term below costs
        // at least `min(acc, lbd)`, and exactly `acc` when `lbd >= acc`.
        let (root_cur, lbd) = ex.root();
        out.stats.rows_computed = 1;
        let acc = ex.prefix_cell(0, &root_cur);
        let lb = acc.min(lbd);
        if lb <= cfg.budget {
            if acc < INF && lbd >= acc {
                self.push_settled(pack(rank(acc), trie.max_weight(0), 0), 0, NO_TERM, acc, 0);
            } else {
                self.push(
                    pack(rank(lb), trie.max_weight(0), 0),
                    0,
                    NO_TERM,
                    acc,
                    0,
                    root_cur,
                    [INF; ROW],
                );
            }
        }

        while let Some(e) = self.heap.pop() {
            if e.term != NO_TERM {
                out.hits.push(Hit {
                    id: e.term,
                    cost: e.cost,
                    weight: trie.weight(e.term),
                });
                if out.hits.len() == cfg.k {
                    break;
                }
                continue;
            }
            if out.stats.nodes_expanded >= cfg.max_nodes {
                out.stats.truncated = true;
                break;
            }
            out.stats.nodes_expanded += 1;
            let v = e.node as usize;
            let depth = usize::from(e.depth);
            let acc = e.cost;

            // A term ending here has had all its prefixes looked at.
            let tid = trie.term_id(v);
            if tid != NO_TERM && acc <= cfg.budget {
                let key = pack(rank(acc), trie.weight(tid), tid);
                self.push_settled(key, e.node, tid, acc, e.depth);
            }

            for c in trie.children(v) {
                if e.settled {
                    self.push_settled(
                        pack(rank(acc), trie.max_weight(c), 0),
                        c as u32,
                        NO_TERM,
                        acc,
                        e.depth + 1,
                    );
                    continue;
                }
                let (row, lbd) = ex.child(v, depth, c, &e.cur, &e.prev);
                out.stats.rows_computed += 1;
                let acc_c = acc.min(ex.prefix_cell(depth + 1, &row));
                let lb = acc_c.min(lbd);
                if lb > cfg.budget {
                    continue;
                }
                if acc_c < INF && lbd >= acc_c {
                    self.push_settled(
                        pack(rank(acc_c), trie.max_weight(c), 0),
                        c as u32,
                        NO_TERM,
                        acc_c,
                        e.depth + 1,
                    );
                } else {
                    self.push(
                        pack(rank(lb), trie.max_weight(c), 0),
                        c as u32,
                        NO_TERM,
                        acc_c,
                        e.depth + 1,
                        row,
                        e.cur,
                    );
                }
            }
        }
        self.qmask = qmask;
        self.qsym = qsym;
        out.stats.nodes_pushed = self.seq as usize;
        Ok(out)
    }
}

/// Direct checks of [`lower_bound`] against a brute-force oracle.
///
/// For every node of random small tries (not only the nodes the search pops),
/// the bound computed through [`Expander`] must not exceed the exact cost of
/// any term at or below the node whose cost is within the budget, the node's
/// queue key must not exceed the key of any such term under either
/// [`Ranking`], and the cost read at a terminal must be the exact cost.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost::Layout;
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;

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

    /// The symbols of a term.
    fn syms(s: &str) -> Vec<u32> {
        s.chars().map(u32::from).collect()
    }

    /// Naive full-matrix weighted OSA cost, independent of the banded rows.
    /// `d[j][i]` aligns the term prefix `t[..j]` with the query prefix `q[..i]`.
    fn oracle(cm: &CostModel, q: &[u32], t: &[u32]) -> u32 {
        oracle_matrix(cm, q, t)[t.len()][q.len()]
    }

    fn oracle_matrix(cm: &CostModel, q: &[u32], t: &[u32]) -> Vec<Vec<u32>> {
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

    fn random_word(rng: &mut Rng, alpha: &[u32], max_len: usize) -> Vec<u32> {
        let len = 1 + rng.below(max_len);
        (0..len).map(|_| alpha[rng.below(alpha.len())]).collect()
    }

    /// Up to `ops` random substitutions, insertions, deletions and swaps.
    fn mutate(rng: &mut Rng, word: &[u32], alpha: &[u32], ops: usize) -> Vec<u32> {
        let mut w = word.to_vec();
        for _ in 0..ops {
            let letter = alpha[rng.below(alpha.len())];
            match rng.below(4) {
                0 if !w.is_empty() => {
                    let i = rng.below(w.len());
                    w[i] = letter;
                }
                1 => {
                    let i = rng.below(w.len() + 1);
                    w.insert(i, letter);
                }
                2 if !w.is_empty() => {
                    let i = rng.below(w.len());
                    w.remove(i);
                }
                3 if w.len() > 1 => {
                    let i = rng.below(w.len() - 1);
                    w.swap(i, i + 1);
                }
                _ => {}
            }
        }
        w
    }

    /// A dictionary of 1 to 24 entries of length 1 to 9 with duplicates,
    /// prefix chains and extensions of earlier entries.
    fn random_dictionary(rng: &mut Rng, alpha: &[u32]) -> Vec<(String, u16)> {
        let size = 1 + rng.below(24);
        let mut words: Vec<Vec<u32>> = Vec::new();
        while words.len() < size {
            let w = match (rng.below(6), words.len()) {
                (0, n) if n > 0 => words[rng.below(n)].clone(),
                (1, n) if n > 0 => {
                    let base = &words[rng.below(n)];
                    base[..1 + rng.below(base.len())].to_vec()
                }
                (2, n) if n > 0 => {
                    let mut w = words[rng.below(n)].clone();
                    for _ in 0..1 + rng.below(3) {
                        if w.len() < 9 {
                            w.push(alpha[rng.below(alpha.len())]);
                        }
                    }
                    w
                }
                _ => random_word(rng, alpha, 9),
            };
            words.push(w);
        }
        words
            .into_iter()
            .map(|w| {
                let weight = if rng.below(3) == 0 {
                    100
                } else {
                    rng.below(65_536) as u16
                };
                let s = w.iter().filter_map(|&c| char::from_u32(c)).collect();
                (s, weight)
            })
            .collect()
    }

    #[derive(Default)]
    struct Coverage {
        instances: usize,
        nodes: usize,
        considered: usize,
        bounded: usize,
        terminals: usize,
        terminals_within: usize,
        settled: usize,
    }

    /// Walks every node of `trie` for one query and checks the bound, the
    /// keys and the terminal costs.
    fn check_instance(
        trie: &Trie,
        cm: &CostModel,
        q: &[u32],
        budget: Cost,
        tsb: bool,
        cov: &mut Coverage,
    ) {
        let w = usize::from(budget / cm.c_indel_min());
        let m = q.len();
        let mut qmask = vec![0u64; m + 1];
        for i in (0..m).rev() {
            qmask[i] = qmask[i + 1] | symbol_class(q[i]);
        }
        let ex = Expander {
            trie,
            cm,
            q,
            qmask: &qmask,
            w,
            budget,
            tsb,
            prefix: false,
        };
        let rankings = [Ranking::Coarse, Ranking::Exact];

        // Exact cost of every term, and per node the smallest within-budget
        // cost and the smallest term key at or below it. Children always have
        // larger indices than their parent.
        let costs: Vec<u32> = (0..trie.len() as u32)
            .map(|id| oracle(cm, q, &syms(trie.term(id))))
            .collect();
        let n = trie.node_count();
        let mut best = vec![u32::MAX; n];
        let mut best_key = [vec![u64::MAX; n], vec![u64::MAX; n]];
        for v in (0..n).rev() {
            let tid = trie.term_id(v);
            if tid != NO_TERM && costs[tid as usize] <= u32::from(budget) {
                let c = costs[tid as usize] as Cost;
                best[v] = u32::from(c);
                for (r, keys) in rankings.iter().zip(best_key.iter_mut()) {
                    keys[v] = pack(r.rank(c), trie.weight(tid), tid);
                }
            }
            for c in trie.children(v) {
                best[v] = best[v].min(best[c]);
                for keys in &mut best_key {
                    keys[v] = keys[v].min(keys[c]);
                }
            }
        }

        let check_node = |v: usize, lb: Cost| {
            if best[v] != u32::MAX {
                assert!(
                    u32::from(lb) <= best[v],
                    "bound {lb} above the cost {} of a term below node {v} \
                     (query {q:?}, budget {budget}, tsb {tsb})",
                    best[v],
                );
                for (r, keys) in rankings.iter().zip(best_key.iter()) {
                    assert!(pack(r.rank(lb), trie.max_weight(v), 0) <= keys[v]);
                }
            }
        };

        cov.instances += 1;
        let (root, lb) = ex.root();
        check_node(0, lb);
        cov.nodes += 1;
        cov.considered += 1;
        cov.bounded += usize::from(best[0] != u32::MAX);
        // (node, depth, row, parent row, whether the search would push it)
        let mut stack = vec![(0usize, 0usize, root, [INF; ROW], lb <= budget)];
        while let Some((v, depth, cur, prev, pushed)) = stack.pop() {
            let tid = trie.term_id(v);
            if tid != NO_TERM {
                let c = costs[tid as usize];
                cov.terminals += 1;
                if c <= u32::from(budget) {
                    cov.terminals_within += 1;
                    let want = Some((tid, c as Cost));
                    assert_eq!(ex.terminal(v, depth, &cur), want, "query {q:?}");
                } else {
                    assert_eq!(ex.terminal(v, depth, &cur), None, "query {q:?}");
                }
            }
            for c in trie.children(v) {
                let (row, lb) = ex.child(v, depth, c, &cur, &prev);
                check_node(c, lb);
                cov.nodes += 1;
                cov.considered += usize::from(pushed);
                cov.bounded += usize::from(best[c] != u32::MAX);
                stack.push((c, depth + 1, row, cur, pushed && lb <= budget));
            }
        }
    }

    /// The alphabets of the property tests: ASCII ones (in the last, '!' and
    /// '"' share the classes of 'a' and 'b').
    const ASCII_ALPHABETS: [&str; 4] = ["aqw", "asdfqwer", "abcdefghijklmnopqrstuvwxyz", "ab!\""];

    /// Non-ASCII alphabets: Latin-1 letters (narrow labels), `à` and `Ć`
    /// (U+00E0 and U+0106, whose classes collide), letters of other scripts
    /// and an emoji (wide labels), and ç with its ABNT2 neighbours.
    const OTHER_ALPHABETS: [&str; 4] = ["aeéèçß", "aàĆc", "жзaЖ😀", "çlp.;"];

    fn check_mode(tsb: bool) -> Coverage {
        check_mode_on(&ASCII_ALPHABETS, tsb)
    }

    fn check_mode_on(alphabets: &[&str], tsb: bool) -> Coverage {
        let mut cov = Coverage::default();
        for (a, alpha) in alphabets.iter().enumerate() {
            let alpha = &syms(alpha);
            for d in 0..150 {
                // Every layout in turn: the bound uses the model's minimum
                // costs, which must hold for any of them.
                let layouts = Layout::ALL;
                let cm = CostModel::for_layout(layouts[d as usize % layouts.len()]);
                let mut rng = Rng::new((a as u64 + 1) * 1000 + d);
                let dict = random_dictionary(&mut rng, alpha);
                let items: Vec<(&str, u16)> = dict.iter().map(|(s, w)| (s.as_str(), *w)).collect();
                let trie = Trie::build(&items).unwrap_or_else(|e| panic!("{e}"));
                for _ in 0..8 {
                    let q = if rng.below(4) == 0 {
                        let len = rng.below(11);
                        (0..len).map(|_| alpha[rng.below(alpha.len())]).collect()
                    } else {
                        let base = &syms(&dict[rng.below(dict.len())].0);
                        let ops = rng.below(4);
                        mutate(&mut rng, base, alpha, ops)
                    };
                    // Band half-widths W = 0 to 6 and 8, with budgets that are not
                    // multiples of 16.
                    for budget in [7, 15, 16, 24, 31, 32, 40, 48, 64] {
                        check_instance(&trie, &cm, &q, budget, tsb, &mut cov);
                    }
                }
            }
        }
        std::eprintln!(
            "tsb {tsb}: {} instances, {} nodes checked ({} considered by the search, \
             {} with a within-budget term below), {} terminals ({} within budget)",
            cov.instances,
            cov.nodes,
            cov.considered,
            cov.bounded,
            cov.terminals,
            cov.terminals_within,
        );
        cov
    }

    #[test]
    fn lower_bound_never_exceeds_the_oracle_without_tsb() {
        let cov = check_mode(false);
        assert!(cov.bounded > 10_000 && cov.terminals_within > 1_000);
    }

    #[test]
    fn lower_bound_never_exceeds_the_oracle_with_tsb() {
        let cov = check_mode(true);
        assert!(cov.bounded > 10_000 && cov.terminals_within > 1_000);
    }

    #[test]
    fn lower_bound_never_exceeds_the_oracle_on_non_ascii_alphabets() {
        for tsb in [false, true] {
            let cov = check_mode_on(&OTHER_ALPHABETS, tsb);
            assert!(cov.bounded > 10_000 && cov.terminals_within > 1_000);
        }
    }

    /// `D[m][j]` for every term prefix length `j` (the last column).
    fn oracle_col(cm: &CostModel, q: &[u32], t: &[u32]) -> Vec<u32> {
        oracle_matrix(cm, q, t).iter().map(|r| r[q.len()]).collect()
    }

    /// Prefix-mode counterpart of `check_instance`. For every node it walks
    /// the same way `search_prefix` does (carrying `acc`) and checks
    ///
    /// - `lbd` (the bound on prefixes strictly longer than the node's) is at
    ///   most `D[m][j']` for every term below and every `j' > depth`, when
    ///   that is within the budget;
    /// - `min(acc, lbd)` and its queue key are at most the prefix cost and key
    ///   of every within-budget term below;
    /// - a settled node (`acc < INF && lbd >= acc`) has every term below at
    ///   exactly cost `acc`;
    /// - at a terminal the cost `acc` equals the oracle prefix cost when that
    ///   is within budget, and no cost is reported otherwise.
    fn check_prefix_instance(
        trie: &Trie,
        cm: &CostModel,
        q: &[u32],
        budget: Cost,
        tsb: bool,
        cov: &mut Coverage,
    ) {
        let w = usize::from(budget / cm.c_indel_min());
        let m = q.len();
        let mut qmask = vec![0u64; m + 1];
        for i in (0..m).rev() {
            qmask[i] = qmask[i + 1] | symbol_class(q[i]);
        }
        let ex = Expander {
            trie,
            cm,
            q,
            qmask: &qmask,
            w,
            budget,
            tsb,
            prefix: true,
        };
        let cols: Vec<Vec<u32>> = (0..trie.len() as u32)
            .map(|id| oracle_col(cm, q, &syms(trie.term(id))))
            .collect();
        let pcost: Vec<u32> = cols
            .iter()
            .map(|c| c.iter().copied().min().unwrap_or(u32::MAX))
            .collect();
        let n = trie.node_count();
        // Terms at or below each node.
        let mut below: Vec<Vec<u32>> = vec![Vec::new(); n];
        for v in (0..n).rev() {
            let tid = trie.term_id(v);
            if tid != NO_TERM {
                below[v].push(tid);
            }
            for c in trie.children(v) {
                let kids = below[c].clone();
                below[v].extend(kids);
            }
        }
        let rankings = [Ranking::Coarse, Ranking::Exact];
        let b32 = u32::from(budget);

        let mut check_node = |v: usize, depth: usize, acc: Cost, lbd: Cost| {
            let mut best = u32::MAX;
            let mut deeper = u32::MAX;
            for &t in &below[v] {
                let col = &cols[t as usize];
                let tail = &col[(depth + 1).min(col.len())..];
                deeper = deeper.min(tail.iter().copied().min().unwrap_or(u32::MAX));
                best = best.min(pcost[t as usize]);
                if pcost[t as usize] <= b32 {
                    let c = pcost[t as usize] as Cost;
                    for r in rankings {
                        let tk = pack(r.rank(c), trie.weight(t), t);
                        let nk = pack(r.rank(acc.min(lbd)), trie.max_weight(v), 0);
                        assert!(nk <= tk, "key above a term below node {v}, q {q:?}");
                    }
                }
            }
            if deeper <= b32 {
                assert!(
                    u32::from(lbd) <= deeper,
                    "deeper bound {lbd} above {deeper} below node {v} at depth {depth} \
                     (query {q:?}, budget {budget}, tsb {tsb})",
                );
            }
            if best <= b32 {
                assert!(
                    u32::from(acc.min(lbd)) <= best,
                    "min(acc, lbd) above {best}"
                );
            }
            if acc < INF && lbd >= acc {
                cov.settled += 1;
                for &t in &below[v] {
                    assert_eq!(
                        pcost[t as usize],
                        u32::from(acc),
                        "settled node {v} has a term of another cost (query {q:?}, budget {budget})"
                    );
                }
            }
            best <= b32
        };

        cov.instances += 1;
        let (root, lbd) = ex.root();
        let acc = ex.prefix_cell(0, &root);
        let b = check_node(0, 0, acc, lbd);
        cov.nodes += 1;
        cov.bounded += usize::from(b);
        let mut stack = vec![(0usize, 0usize, root, [INF; ROW], acc)];
        while let Some((v, depth, cur, prev, acc)) = stack.pop() {
            let tid = trie.term_id(v);
            if tid != NO_TERM {
                cov.terminals += 1;
                let c = pcost[tid as usize];
                if c <= b32 {
                    cov.terminals_within += 1;
                    assert_eq!(u32::from(acc), c, "terminal cost, query {q:?}");
                } else {
                    assert!(acc > budget, "terminal reported beyond the budget");
                }
            }
            for c in trie.children(v) {
                let (row, lbd) = ex.child(v, depth, c, &cur, &prev);
                let acc_c = acc.min(ex.prefix_cell(depth + 1, &row));
                let b = check_node(c, depth + 1, acc_c, lbd);
                cov.nodes += 1;
                cov.bounded += usize::from(b);
                stack.push((c, depth + 1, row, cur, acc_c));
            }
        }
    }

    fn check_prefix_mode(tsb: bool) -> Coverage {
        check_prefix_mode_on(&ASCII_ALPHABETS, tsb, false)
    }

    /// With `all_layouts`, dictionary `d` uses layout `d % 6` (the ASCII run
    /// keeps QWERTY, so that the counts quoted in `prefix-mode.md` hold).
    fn check_prefix_mode_on(alphabets: &[&str], tsb: bool, all_layouts: bool) -> Coverage {
        let mut cov = Coverage::default();
        for (a, alpha) in alphabets.iter().enumerate() {
            let alpha = &syms(alpha);
            for d in 0..150 {
                let cm = if all_layouts {
                    CostModel::for_layout(Layout::ALL[d as usize % Layout::ALL.len()])
                } else {
                    CostModel::qwerty()
                };
                let mut rng = Rng::new((a as u64 + 1) * 1000 + d);
                let dict = random_dictionary(&mut rng, alpha);
                let items: Vec<(&str, u16)> = dict.iter().map(|(s, w)| (s.as_str(), *w)).collect();
                let trie = Trie::build(&items).unwrap_or_else(|e| panic!("{e}"));
                for _ in 0..8 {
                    let q = if rng.below(4) == 0 {
                        let len = rng.below(11);
                        (0..len).map(|_| alpha[rng.below(alpha.len())]).collect()
                    } else {
                        // A truncated dictionary entry with 0 to 3 edits.
                        let base = &syms(&dict[rng.below(dict.len())].0);
                        let cut = 1 + rng.below(base.len());
                        let ops = rng.below(4);
                        mutate(&mut rng, &base[..cut], alpha, ops)
                    };
                    for budget in [7, 15, 16, 24, 31, 32, 40, 48, 64] {
                        check_prefix_instance(&trie, &cm, &q, budget, tsb, &mut cov);
                    }
                }
            }
        }
        std::eprintln!(
            "prefix, tsb {tsb}: {} instances, {} nodes checked ({} with a within-budget \
             term below, {} settled), {} terminals ({} within budget)",
            cov.instances,
            cov.nodes,
            cov.bounded,
            cov.settled,
            cov.terminals,
            cov.terminals_within,
        );
        cov
    }

    #[test]
    fn prefix_bound_never_exceeds_the_oracle_without_tsb() {
        let cov = check_prefix_mode(false);
        assert!(cov.bounded > 10_000 && cov.terminals_within > 1_000 && cov.settled > 1_000);
    }

    #[test]
    fn prefix_bound_never_exceeds_the_oracle_with_tsb() {
        let cov = check_prefix_mode(true);
        assert!(cov.bounded > 10_000 && cov.terminals_within > 1_000 && cov.settled > 1_000);
    }

    #[test]
    fn prefix_bound_never_exceeds_the_oracle_on_non_ascii_alphabets() {
        for tsb in [false, true] {
            let cov = check_prefix_mode_on(&OTHER_ALPHABETS, tsb, true);
            assert!(cov.bounded > 10_000 && cov.terminals_within > 1_000 && cov.settled > 1_000);
        }
    }
}

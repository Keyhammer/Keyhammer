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

use alloc::collections::BinaryHeap;
use alloc::vec::Vec;
use core::cmp::Ordering;
use core::fmt;

use crate::cost::{Cost, CostModel, INF, class, whole_units};
use crate::trie::{NO_TERM, Trie};

/// Longest accepted query, in bytes.
pub const MAX_QUERY_LEN: usize = 128;
const MAX_W: usize = 8;
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
/// defaults: `k = 10`, `budget = 32` (two ordinary edits' worth of cost), `tsb = false`,
/// `max_nodes = 100_000`, `ranking = Ranking::Coarse`.
#[derive(Clone, Debug)]
pub struct SearchConfig {
    /// Number of results wanted.
    pub k: usize,
    /// Largest accepted edit cost (fixed point, 16 = one edit).
    pub budget: Cost,
    /// Use the subtree-signature lower bound.
    pub tsb: bool,
    /// Hard limit on expanded trie nodes.
    pub max_nodes: usize,
    /// How results are ordered.
    pub ranking: Ranking,
}

impl SearchConfig {
    /// An opt-in configuration that trades speed for recall: the default
    /// settings with `budget = 48` (three ordinary edits' worth of cost).
    ///
    /// On the benchmark in `docs/benchmarks/recall-preset.md` (300 Birkbeck
    /// typo pairs, one machine, one run) it found the right word more often
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
    /// ```
    #[must_use]
    pub fn high_recall() -> Self {
        Self {
            budget: 48,
            ..Self::default()
        }
    }
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            k: 10,
            budget: 32,
            tsb: false,
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Trie nodes expanded.
    pub nodes_expanded: usize,
    /// Queue entries created (nodes and terminals).
    pub nodes_pushed: usize,
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
        /// Query length in bytes.
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
                write!(f, "query has {len} bytes, the limit is {max}")
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
    /// Exact weighted cost of a terminal entry (unused for node entries).
    cost: Cost,
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

fn root_row(q: &[u8], cm: &CostModel, w: usize, budget: Cost) -> [Cost; ROW] {
    let mut row = [INF; ROW];
    row[w] = 0;
    for i in 1..=q.len().min(w) {
        row[w + i] = cap(row[w + i - 1].saturating_add(cm.del_cost(q, i - 1)), budget);
    }
    row
}

#[allow(clippy::too_many_arguments)]
fn child_row(
    q: &[u8],
    cm: &CostModel,
    w: usize,
    budget: Cost,
    depth: usize,
    parent_label: u8,
    ch: u8,
    cur: &[Cost; ROW],
    prev: &[Cost; ROW],
) -> [Cost; ROW] {
    let m = q.len();
    let j = depth + 1;
    let mut out = [INF; ROW];
    for k in 0..=2 * w {
        let i_signed = (j + k) as isize - w as isize;
        if i_signed < 0 || i_signed as usize > m {
            continue;
        }
        let i = i_signed as usize;
        let mut best = INF;
        if i >= 1 && cur[k] < INF {
            best = best.min(cur[k] + cm.sub_cost(q[i - 1], ch, i - 1));
        }
        if k < 2 * w && cur[k + 1] < INF {
            let prev_t = if depth >= 1 { Some(parent_label) } else { None };
            best = best.min(cur[k + 1] + cm.ins_cost(ch, prev_t, i));
        }
        if i >= 1 && k >= 1 && out[k - 1] < INF {
            best = best.min(out[k - 1] + cm.del_cost(q, i - 1));
        }
        if depth >= 1
            && i >= 2
            && q[i - 1] == parent_label
            && q[i - 2] == ch
            && q[i - 1] != q[i - 2]
            && prev[k] < INF
        {
            best = best.min(prev[k] + cm.transpose_cost(i - 2));
        }
        out[k] = cap(best, budget);
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
            let gap = if r < lo { lo - r } else { r.saturating_sub(hi) };
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
    q: &'a [u8],
    qmask: &'a [u64],
    w: usize,
    budget: Cost,
    tsb: bool,
}

impl Expander<'_> {
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
            self.trie.label(v),
            self.trie.label(c),
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
    seq: u32,
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
            depth,
            cur,
            prev,
        });
    }

    /// Returns the `cfg.k` best terms within `cfg.budget`, best first, in the
    /// order set by `cfg.ranking`.
    ///
    /// `q` must already be lowercase; bytes outside `a..=z` are compared verbatim.
    pub fn search(
        &mut self,
        trie: &Trie,
        cm: &CostModel,
        q: &[u8],
        cfg: &SearchConfig,
    ) -> Result<Output, SearchError> {
        if q.len() > MAX_QUERY_LEN {
            return Err(SearchError::QueryTooLong {
                len: q.len(),
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
        let w = usize::from(cfg.budget / cm.c_indel_min());
        let mut out = Output {
            hits: Vec::new(),
            stats: Stats::default(),
        };
        if cfg.k == 0 {
            return Ok(out);
        }
        let m = q.len();
        self.qmask.clear();
        self.qmask.resize(m + 1, 0);
        for i in (0..m).rev() {
            self.qmask[i] = self.qmask[i + 1] | class(q[i]);
        }
        self.heap.clear();
        self.seq = 0;

        // Moved out for the duration of the search so that the expander can
        // borrow it while entries are pushed; put back below.
        let qmask = core::mem::take(&mut self.qmask);
        let ex = Expander {
            trie,
            cm,
            q,
            qmask: &qmask,
            w,
            budget: cfg.budget,
            tsb: cfg.tsb,
        };
        let rank = |c: Cost| cfg.ranking.rank(c);
        let (root_cur, lb) = ex.root();
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

    /// Naive full-matrix weighted OSA cost, independent of the banded rows.
    /// `d[j][i]` aligns the term prefix `t[..j]` with the query prefix `q[..i]`.
    fn oracle(cm: &CostModel, q: &[u8], t: &[u8]) -> u32 {
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
        d[n][m]
    }

    fn random_word(rng: &mut Rng, alpha: &[u8], max_len: usize) -> Vec<u8> {
        let len = 1 + rng.below(max_len);
        (0..len).map(|_| alpha[rng.below(alpha.len())]).collect()
    }

    /// Up to `ops` random substitutions, insertions, deletions and swaps.
    fn mutate(rng: &mut Rng, word: &[u8], alpha: &[u8], ops: usize) -> Vec<u8> {
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
    fn random_dictionary(rng: &mut Rng, alpha: &[u8]) -> Vec<(String, u16)> {
        let size = 1 + rng.below(24);
        let mut words: Vec<Vec<u8>> = Vec::new();
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
                (String::from_utf8(w).unwrap_or_default(), weight)
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
    }

    /// Walks every node of `trie` for one query and checks the bound, the
    /// keys and the terminal costs.
    fn check_instance(
        trie: &Trie,
        cm: &CostModel,
        q: &[u8],
        budget: Cost,
        tsb: bool,
        cov: &mut Coverage,
    ) {
        let w = usize::from(budget / cm.c_indel_min());
        let m = q.len();
        let mut qmask = vec![0u64; m + 1];
        for i in (0..m).rev() {
            qmask[i] = qmask[i + 1] | class(q[i]);
        }
        let ex = Expander {
            trie,
            cm,
            q,
            qmask: &qmask,
            w,
            budget,
            tsb,
        };
        let rankings = [Ranking::Coarse, Ranking::Exact];

        // Exact cost of every term, and per node the smallest within-budget
        // cost and the smallest term key at or below it. Children always have
        // larger indices than their parent.
        let costs: Vec<u32> = (0..trie.len() as u32)
            .map(|id| oracle(cm, q, trie.term(id).as_bytes()))
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

    fn check_mode(tsb: bool) -> Coverage {
        // In the last alphabet '!' and '"' share the classes of 'a' and 'b'.
        let alphabets: [&[u8]; 4] = [b"aqw", b"asdfqwer", b"abcdefghijklmnopqrstuvwxyz", b"ab!\""];
        let cm = CostModel::qwerty();
        let mut cov = Coverage::default();
        for (a, alpha) in alphabets.iter().enumerate() {
            for d in 0..150 {
                let mut rng = Rng::new((a as u64 + 1) * 1000 + d);
                let dict = random_dictionary(&mut rng, alpha);
                let items: Vec<(&str, u16)> = dict.iter().map(|(s, w)| (s.as_str(), *w)).collect();
                let trie = Trie::build(&items).unwrap_or_else(|e| panic!("{e}"));
                for _ in 0..8 {
                    let q = if rng.below(4) == 0 {
                        let len = rng.below(11);
                        (0..len).map(|_| alpha[rng.below(alpha.len())]).collect()
                    } else {
                        let base = dict[rng.below(dict.len())].0.as_bytes();
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
}

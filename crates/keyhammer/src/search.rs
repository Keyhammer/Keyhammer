// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! Exact best-first top-k search over a [`Trie`] with weighted edit costs.
//!
//! Each queued trie node carries one banded row of the edit-distance matrix.
//! A row cell `k` at trie depth `j` stands for query prefix length
//! `i = j + k - W`, where `W = budget / c_indel_min` is the band half-width.
//! A node is queued with a lower bound on the cost of every term below it, so
//! the first terminal popped is always the best remaining one.

use alloc::collections::BinaryHeap;
use alloc::vec::Vec;
use core::cmp::Ordering;
use core::fmt;

use crate::cost::{Cost, CostModel, INF, class};
use crate::trie::{NO_TERM, Trie};

/// Longest accepted query, in bytes.
pub const MAX_QUERY_LEN: usize = 128;
const MAX_W: usize = 8;
const ROW: usize = 2 * MAX_W + 1;

/// Search parameters.
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
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            k: 10,
            budget: 32,
            tsb: false,
            max_nodes: 100_000,
        }
    }
}

/// One result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    /// Term id (see [`Trie::term`]).
    pub id: u32,
    /// Weighted edit cost between the query and the term.
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

#[inline]
fn pack(cost: Cost, weight: u16, id: u32) -> u64 {
    (u64::from(cost) << 48) | (u64::from(u16::MAX - weight) << 32) | u64::from(id)
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

    fn push(
        &mut self,
        key: u64,
        node: u32,
        term: u32,
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
            depth,
            cur,
            prev,
        });
    }

    /// Returns the `cfg.k` terms of lowest cost within `cfg.budget`, best first.
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

        let root_cur = root_row(q, cm, w, cfg.budget);
        let root_prev = [INF; ROW];
        let lb = lower_bound(
            trie,
            0,
            0,
            &root_cur,
            &root_prev,
            m,
            w,
            &self.qmask,
            cm,
            cfg.tsb,
        );
        if lb <= cfg.budget {
            self.push(
                pack(lb, trie.max_weight(0), 0),
                0,
                NO_TERM,
                0,
                root_cur,
                root_prev,
            );
        }

        while let Some(e) = self.heap.pop() {
            if e.term != NO_TERM {
                out.hits.push(Hit {
                    id: e.term,
                    cost: (e.key >> 48) as Cost,
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

            let tid = trie.term_id(v);
            if tid != NO_TERM {
                if let Some(k) = (m + w).checked_sub(depth) {
                    if k <= 2 * w && e.cur[k] <= cfg.budget {
                        let key = pack(e.cur[k], trie.weight(tid), tid);
                        self.push(key, e.node, tid, e.depth, [INF; ROW], [INF; ROW]);
                    }
                }
            }

            let parent_label = trie.label(v);
            for c in trie.children(v) {
                let ch = trie.label(c);
                let row = child_row(
                    q,
                    cm,
                    w,
                    cfg.budget,
                    depth,
                    parent_label,
                    ch,
                    &e.cur,
                    &e.prev,
                );
                let lb = lower_bound(
                    trie,
                    c,
                    depth + 1,
                    &row,
                    &e.cur,
                    m,
                    w,
                    &self.qmask,
                    cm,
                    cfg.tsb,
                );
                if lb <= cfg.budget {
                    self.push(
                        pack(lb, trie.max_weight(c), 0),
                        c as u32,
                        NO_TERM,
                        e.depth + 1,
                        row,
                        e.cur,
                    );
                }
            }
        }
        out.stats.nodes_pushed = self.seq as usize;
        Ok(out)
    }
}

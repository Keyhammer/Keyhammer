// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! Research (#44): is a SUM of the two subtree-signature terms admissible?
//!
//! A self-contained reference of the node bound (full-matrix rows, no trie
//! and no banded storage), used only to compare `max` and `sum` against the
//! weighted-OSA oracle. It re-implements the bound of `lower_bound` in
//! `src/search.rs`; the production code is not touched. See
//! `docs/benchmarks/sum-bound.md`.

#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

mod support;

use keyhammer::cost::{Cost, CostModel, class};
use support::{Rng, oracle_cost};

const BIG: u32 = 1_000_000;

/// Full DP matrix `d[j][i]` (term prefix length j, query prefix length i).
fn matrix(cm: &CostModel, q: &[u8], t: &[u8]) -> Vec<Vec<u32>> {
    tail_matrix(cm, q, t, 0, 0, false)
}

/// DP from the single source cell `(i0, j0)`; `no_first_del` forbids a
/// deletion as the first step out of the source (the situation of the last
/// cell a path visits in row `j0`).
fn tail_matrix(
    cm: &CostModel,
    q: &[u8],
    t: &[u8],
    i0: usize,
    j0: usize,
    no_first_del: bool,
) -> Vec<Vec<u32>> {
    let (m, n) = (q.len(), t.len());
    let mut d = vec![vec![BIG; m + 1]; n + 1];
    d[j0][i0] = 0;
    for j in j0..=n {
        for i in i0..=m {
            if i == i0 && j == j0 {
                continue;
            }
            let mut best = BIG;
            if i > i0 && j > j0 {
                best =
                    best.min(d[j - 1][i - 1] + u32::from(cm.sub_cost(q[i - 1], t[j - 1], i - 1)));
            }
            if j > j0 {
                let pt = if j >= 2 { Some(t[j - 2]) } else { None };
                best = best.min(d[j - 1][i] + u32::from(cm.ins_cost(t[j - 1], pt, i)));
            }
            if i > i0 && !(no_first_del && i - 1 == i0 && j == j0) {
                best = best.min(d[j][i - 1] + u32::from(cm.del_cost(q, i - 1)));
            }
            if i >= i0 + 2
                && j >= j0 + 2
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

fn tail(cm: &CostModel, q: &[u8], t: &[u8], i0: usize, j0: usize, no_first_del: bool) -> u32 {
    tail_matrix(cm, q, t, i0, j0, no_first_del)[t.len()][q.len()]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Max,
    Sum,
}

fn qmask(q: &[u8]) -> Vec<u64> {
    let mut v = vec![0u64; q.len() + 1];
    for i in (0..q.len()).rev() {
        v[i] = v[i + 1] | class(q[i]);
    }
    v
}

struct Sig {
    len_min: usize,
    len_max: usize,
    below: u64,
}

/// `(T_comp, T_let)` of cell `(i, j)`, exactly as `lower_bound` computes them.
fn terms(cm: &CostModel, qm: &[u64], m: usize, i: usize, j: usize, s: &Sig) -> (u32, u32) {
    let r = m - i;
    let lo = s.len_min - j;
    let hi = s.len_max - j;
    let gap = if r < lo { lo - r } else { r.saturating_sub(hi) };
    let comp = gap as u32 * u32::from(cm.c_indel_min());
    let miss = (qm[i] & !s.below).count_ones() * u32::from(cm.c_min());
    (comp, miss)
}

/// A witness that a bound exceeded a true cost (read through `Debug`).
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct Witness {
    dict: Vec<String>,
    query: String,
    budget: Cost,
    node: String,
    term: String,
    bound: u32,
    cost: u32,
}

/// Cell-level violation counters: a cell is violated when its extra term
/// exceeds the true cost of the rest of the path to some term below it.
#[derive(Default, Clone, Copy, Debug)]
struct Cells {
    /// any tail
    all: u64,
    /// tails whose first step is not a deletion (last cell in the row)
    last_in_row: u64,
}

/// Node-level check: for every prefix `v` of the dictionary and every term
/// below it within the budget, `LB(v) <= cost(t)`. Returns the first violation.
fn check(
    cm: &CostModel,
    dict: &[&[u8]],
    q: &[u8],
    budget: Cost,
    mode: Mode,
    cells: Option<&mut Cells>,
) -> Option<Witness> {
    let m = q.len();
    let w = usize::from(budget / cm.c_indel_min());
    let qm = qmask(q);
    let mut prefixes: Vec<&[u8]> = vec![&[]];
    for t in dict {
        for l in 1..=t.len() {
            prefixes.push(&t[..l]);
        }
    }
    prefixes.sort();
    prefixes.dedup();
    let mut cells = cells;
    let mut first = None;
    for p in prefixes {
        let j = p.len();
        let below: Vec<&[u8]> = dict.iter().filter(|t| t.starts_with(p)).copied().collect();
        let sig = Sig {
            len_min: below.iter().map(|t| t.len()).min().unwrap_or(0),
            len_max: below.iter().map(|t| t.len()).max().unwrap_or(0),
            below: below
                .iter()
                .flat_map(|t| t[j..].iter())
                .fold(0, |a, &b| a | class(b)),
        };
        let dmat = matrix(cm, q, p);
        let mut lb = u32::MAX;
        for i in 0..=m {
            let d = dmat[j][i];
            if d > u32::from(budget) || i.abs_diff(j) > w {
                continue;
            }
            let (c, l) = terms(cm, &qm, m, i, j, &sig);
            let extra = match mode {
                Mode::Max => c.max(l),
                Mode::Sum => c + l,
            };
            lb = lb.min(d + extra);
            if let Some(cs) = cells.as_deref_mut() {
                for t in &below {
                    if c + l > tail(cm, q, t, i, j, false) {
                        cs.all += 1;
                    }
                    if c + l > tail(cm, q, t, i, j, true) {
                        cs.last_in_row += 1;
                    }
                }
            }
        }
        if j >= 1 {
            let pm = matrix(cm, q, &p[..j - 1]);
            for i in 0..=m {
                let d = pm[j - 1][i];
                if d <= u32::from(budget) && i.abs_diff(j - 1) <= w {
                    lb = lb.min(d + u32::from(cm.c_transpose_min()));
                }
            }
        }
        let best = below
            .iter()
            .map(|t| (oracle_cost(cm, q, t), *t))
            .filter(|&(c, _)| c <= u32::from(budget))
            .min_by_key(|&(c, _)| c);
        if let Some((c, t)) = best {
            if lb > c && first.is_none() {
                first = Some(Witness {
                    dict: dict
                        .iter()
                        .map(|t| String::from_utf8_lossy(t).into())
                        .collect(),
                    query: String::from_utf8_lossy(q).into(),
                    budget,
                    node: String::from_utf8_lossy(p).into(),
                    term: String::from_utf8_lossy(t).into(),
                    bound: lb,
                    cost: c,
                });
            }
        }
    }
    first
}

fn strings(alpha: &[u8], max_len: usize) -> Vec<Vec<u8>> {
    let mut out = vec![vec![]];
    let mut layer: Vec<Vec<u8>> = vec![vec![]];
    for _ in 0..max_len {
        let mut next = Vec::new();
        for s in &layer {
            for &a in alpha {
                let mut t = s.clone();
                t.push(a);
                next.push(t);
            }
        }
        out.extend(next.iter().cloned());
        layer = next;
    }
    out
}

fn subsets(n: usize, k: usize) -> Vec<Vec<usize>> {
    fn rec(start: usize, n: usize, k: usize, cur: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if !cur.is_empty() {
            out.push(cur.clone());
        }
        if cur.len() == k {
            return;
        }
        for i in start..n {
            cur.push(i);
            rec(i + 1, n, k, cur, out);
            cur.pop();
        }
    }
    let mut out = Vec::new();
    rec(0, n, k, &mut Vec::new(), &mut out);
    out
}

fn size(w: &Witness) -> usize {
    w.dict.iter().map(|t| t.len() + 1).sum::<usize>() + w.query.len()
}

#[derive(Default)]
struct Tally {
    cases: u64,
    max_bad: u64,
    sum_bad: u64,
    cells: Cells,
    minimal_sum: Option<Witness>,
}

impl Tally {
    fn case(&mut self, cm: &CostModel, dict: &[&[u8]], q: &[u8], b: Cost, cells: bool) {
        self.cases += 1;
        if check(cm, dict, q, b, Mode::Max, None).is_some() {
            self.max_bad += 1;
        }
        let mut c = Cells::default();
        if let Some(wit) = check(cm, dict, q, b, Mode::Sum, cells.then_some(&mut c)) {
            self.sum_bad += 1;
            if self
                .minimal_sum
                .as_ref()
                .is_none_or(|m| size(&wit) < size(m))
            {
                self.minimal_sum = Some(wit);
            }
        }
        self.cells.all += c.all;
        self.cells.last_in_row += c.last_in_row;
    }
}

const BUDGETS: [Cost; 7] = [7, 15, 16, 24, 32, 48, 64];

#[test]
#[ignore = "research sweep (#44): cargo test -p keyhammer --test sum_bound --release -- --ignored --nocapture"]
fn sweep() {
    let cm = CostModel::qwerty();
    for (alpha, tl, ql, k) in [
        (&b"as"[..], 4, 5, 3),
        (&b"ax"[..], 4, 5, 3),
        (&b"asx"[..], 3, 4, 3),
        (&b"asd"[..], 3, 4, 2),
    ] {
        let terms: Vec<Vec<u8>> = strings(alpha, tl)
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect();
        let queries = strings(alpha, ql);
        let mut t = Tally::default();
        for ix in subsets(terms.len(), k) {
            let dict: Vec<&[u8]> = ix.iter().map(|&i| terms[i].as_slice()).collect();
            for q in &queries {
                for &b in &BUDGETS {
                    t.case(&cm, &dict, q, b, true);
                }
            }
        }
        println!(
            "alphabet {:?}: cases={} max_bad={} sum_bad={} cell_viol={:?}",
            String::from_utf8_lossy(alpha),
            t.cases,
            t.max_bad,
            t.sum_bad,
            t.cells
        );
        if let Some(w) = t.minimal_sum {
            println!("  minimal sum witness: {w:?}");
        }
    }
    // random, larger
    let mut rng = Rng::new(44);
    let mut t = Tally::default();
    for round in 0..40_000u64 {
        let alpha: &[u8] = [&b"aqw"[..], b"asdfqwer", b"ax", b"aab"][(round % 4) as usize];
        let nd = 1 + rng.below(6) as usize;
        let dict: Vec<Vec<u8>> = (0..nd)
            .map(|_| {
                (0..1 + rng.below(8))
                    .map(|_| alpha[rng.below(alpha.len() as u64) as usize])
                    .collect()
            })
            .collect();
        let refs: Vec<&[u8]> = dict.iter().map(|s| s.as_slice()).collect();
        for _ in 0..4 {
            let q: Vec<u8> = if rng.below(2) == 0 {
                (0..rng.below(10))
                    .map(|_| alpha[rng.below(alpha.len() as u64) as usize])
                    .collect()
            } else {
                let mut q = dict[rng.below(nd as u64) as usize].clone();
                for _ in 0..rng.below(4) {
                    let a = alpha[rng.below(alpha.len() as u64) as usize];
                    match rng.below(3) {
                        0 if !q.is_empty() => {
                            let p = rng.below(q.len() as u64) as usize;
                            q[p] = a;
                        }
                        1 => {
                            let p = rng.below(q.len() as u64 + 1) as usize;
                            q.insert(p, a);
                        }
                        _ if !q.is_empty() => {
                            let p = rng.below(q.len() as u64) as usize;
                            q.remove(p);
                        }
                        _ => {}
                    }
                }
                q
            };
            let b = BUDGETS[rng.below(BUDGETS.len() as u64) as usize];
            t.case(&cm, &refs, &q, b, round % 8 == 0);
        }
    }
    println!(
        "random: cases={} max_bad={} sum_bad={} cell_viol={:?}",
        t.cases, t.max_bad, t.sum_bad, t.cells
    );
    if let Some(w) = t.minimal_sum {
        println!("  minimal sum witness: {w:?}");
    }
}

/// Small, fast version of the sweep that runs in CI.
#[test]
#[cfg_attr(miri, ignore = "brute force; no UB surface")]
fn the_sum_is_admissible_on_a_small_exhaustive_sweep() {
    let cm = CostModel::qwerty();
    let terms: Vec<Vec<u8>> = strings(b"as", 3)
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    let queries = strings(b"as", 4);
    let mut t = Tally::default();
    for ix in subsets(terms.len(), 2) {
        let dict: Vec<&[u8]> = ix.iter().map(|&i| terms[i].as_slice()).collect();
        for q in &queries {
            for &b in &[15, 24, 48] {
                t.case(&cm, &dict, q, b, true);
            }
        }
    }
    assert!(t.cases > 5_000);
    assert_eq!(t.max_bad, 0, "max must stay admissible");
    assert_eq!(
        t.sum_bad, 0,
        "counterexample to the sum: {:?}",
        t.minimal_sum
    );
    // The cell-level claim the proof needs (the last cell of a path in a row).
    assert_eq!(t.cells.last_in_row, 0);
    // The claim is false for a tail that starts with a deletion: a doubled
    // letter makes that deletion cost 8 although both terms charge 8 for it
    // (dictionary {"a"}, query "aa", cell (1, 1)). The proof does not use
    // such tails, and the node bound never depends on them.
    assert!(t.cells.all > 0);
}

/// The minimal shape of the doubled-letter overlap, checked by hand.
#[test]
fn a_tail_that_starts_with_a_doubled_deletion_breaks_the_cell_claim_but_not_the_node_bound() {
    let cm = CostModel::qwerty();
    let (q, t): (&[u8], &[u8]) = (b"aa", b"a");
    // cell (i, j) = (1, 1): T_comp = 8 (one query byte left, none below), T_let = 8
    let qm = qmask(q);
    let sig = Sig {
        len_min: 1,
        len_max: 1,
        below: 0,
    };
    assert_eq!(terms(&cm, &qm, 2, 1, 1, &sig), (8, 8));
    // the rest of the path is the deletion of the doubled `a`: 8 < 16
    assert_eq!(tail(&cm, q, t, 1, 1, false), 8);
    // the last cell of the path in row 1 is (2, 1), where nothing is missing
    assert_eq!(tail(&cm, q, t, 1, 1, true), BIG);
    assert_eq!(terms(&cm, &qm, 2, 2, 1, &sig), (0, 0));
    let refs: Vec<&[u8]> = vec![t];
    for mode in [Mode::Max, Mode::Sum] {
        assert!(check(&cm, &refs, q, 32, mode, None).is_none());
    }
}

/// The cell that would break the sum under a cheaper non-doubled indel (12):
/// with the real table the sum is tight, not exceeded.
#[test]
fn the_sum_is_tight_at_the_root_of_b_and_bxy() {
    let cm = CostModel::qwerty();
    let (q, t): (&[u8], &[u8]) = (b"bxy", b"b");
    let qm = qmask(q);
    let sig = Sig {
        len_min: 1,
        len_max: 1,
        below: class(b'b'),
    };
    // T_comp = 2 * 8 (two extra query bytes), T_let = 2 * 8 (x and y)
    assert_eq!(terms(&cm, &qm, 3, 0, 0, &sig), (16, 16));
    // b=b, delete x (16), delete y (16): equal to the sum. With indel = 12
    // it would be 24 < 32 and the sum would not be admissible.
    assert_eq!(oracle_cost(&cm, q, t), 32);
    assert_eq!(tail(&cm, q, t, 0, 0, true), 32);
}

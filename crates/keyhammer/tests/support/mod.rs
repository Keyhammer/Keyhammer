// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
#![allow(dead_code)]

use keyhammer::cost::{Cost, CostModel};
use keyhammer::trie::Trie;

/// Small deterministic xorshift generator (no dependencies).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(2_654_435_761).wrapping_add(1))
    }
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Naive full-matrix weighted OSA distance, written independently of the trie search.
pub fn oracle_cost(cm: &CostModel, q: &[u8], t: &[u8]) -> u32 {
    const BIG: u32 = 1_000_000;
    let (m, n) = (q.len(), t.len());
    let mut d = vec![vec![BIG; m + 1]; n + 1];
    d[0][0] = 0;
    for j in 0..=n {
        for i in 0..=m {
            if j == 0 && i == 0 {
                continue;
            }
            let mut best = BIG;
            if j >= 1 && i >= 1 {
                let c = u32::from(cm.sub_cost(q[i - 1], t[j - 1], i - 1));
                best = best.min(d[j - 1][i - 1] + c);
            }
            if j >= 1 {
                let prev_t = if j >= 2 { Some(t[j - 2]) } else { None };
                best = best.min(d[j - 1][i] + u32::from(cm.ins_cost(t[j - 1], prev_t, i)));
            }
            if i >= 1 {
                best = best.min(d[j][i - 1] + u32::from(cm.del_cost(q, i - 1)));
            }
            if j >= 2
                && i >= 2
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

/// Exact top-k by brute force: (id, cost) ordered by (cost, 65535 - weight, id).
pub fn oracle_topk(
    trie: &Trie,
    cm: &CostModel,
    q: &[u8],
    budget: Cost,
    k: usize,
) -> Vec<(u32, Cost)> {
    let mut all: Vec<(u32, u32, u32)> = Vec::new();
    for id in 0..trie.len() as u32 {
        let c = oracle_cost(cm, q, trie.term(id).as_bytes());
        if c <= u32::from(budget) {
            all.push((c, 65_535 - u32::from(trie.weight(id)), id));
        }
    }
    all.sort();
    all.into_iter()
        .take(k)
        .map(|(c, _, id)| (id, c as Cost))
        .collect()
}

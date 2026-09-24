// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

#![allow(dead_code)]
/// Fiat-Naor function inversion (Section 4.3 of arXiv:2604.01307).
///
/// Given f: [n] → [n], build a structure to compute f⁻¹(j) in O(σ³) time
/// using O(n/σ) space. Uses chained hashing across σ² clusters.

use std::collections::{HashMap, HashSet};

pub struct FunctionInverter {
    n: usize,
    sigma: usize,
    clusters: Vec<Cluster>,
    /// Elements not covered by any chain — stored directly for correctness.
    missing: HashMap<usize, Vec<usize>>,
}

struct Cluster {
    seed: u64,
    chain_ends: HashMap<usize, usize>, // endpoint → chain start
}

impl FunctionInverter {
    /// Build the inverter for function `f` over domain [0, n).
    /// `sigma` controls the space/time tradeoff.
    pub fn build(n: usize, sigma: usize, f: &dyn Fn(usize) -> Option<usize>) -> Self {
        let num_clusters = (sigma * sigma).max(1);
        let chains_per_cluster = (n / (sigma.pow(3)).max(1)).max(1);

        let mut clusters = Vec::with_capacity(num_clusters);
        let mut found = HashSet::new();

        for c in 0..num_clusters {
            let seed = (c as u64).wrapping_mul(2654435761);
            let mut chain_ends = HashMap::new();

            for t in 0..chains_per_cluster {
                let x = ((seed.wrapping_mul((t + 1) as u64).wrapping_mul(1000003)) % n as u64) as usize;
                let mut cur = x;

                for _ in 0..sigma {
                    match f(cur) {
                        Some(fv) => {
                            found.insert(cur);
                            cur = hash(seed, fv, n);
                        }
                        None => break,
                    }
                }

                chain_ends.insert(cur, x);
            }

            clusters.push(Cluster { seed, chain_ends });
        }

        // store elements not covered by any chain
        let mut missing: HashMap<usize, Vec<usize>> = HashMap::new();
        for i in 0..n {
            if !found.contains(&i) {
                if let Some(j) = f(i) {
                    missing.entry(j).or_default().push(i);
                }
            }
        }

        Self { n, sigma, clusters, missing }
    }

    /// Find all i such that f(i) = j.
    pub fn invert(&self, j: usize, f: &dyn Fn(usize) -> Option<usize>) -> Vec<usize> {
        let mut results = HashSet::new();

        if let Some(m) = self.missing.get(&j) {
            results.extend(m);
        }

        for cluster in &self.clusters {
            let mut cur = j;
            let mut chain = vec![cur];
            for _ in 0..self.sigma {
                cur = hash(cluster.seed, cur, self.n);
                chain.push(cur);
            }

            for endpoint in &chain {
                if let Some(&x) = cluster.chain_ends.get(endpoint) {
                    let mut curr = x;
                    for _ in 0..self.sigma {
                        match f(curr) {
                            Some(fv) => {
                                if fv == j {
                                    results.insert(curr);
                                }
                                curr = hash(cluster.seed, fv, self.n);
                            }
                            None => break,
                        }
                    }
                    break;
                }
            }
        }

        results.into_iter().collect()
    }
}

fn hash(seed: u64, val: usize, n: usize) -> usize {
    (seed.wrapping_mul(val as u64 + 1).wrapping_mul(2246822519) % n as u64) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_inversion() {
        // f(i) = i / 3 (groups of 3 map to same value)
        let n = 12;
        let f = |i: usize| -> Option<usize> { Some(i / 3) };
        let inv = FunctionInverter::build(n, 3, &f);

        let preimage = inv.invert(1, &f); // f⁻¹(1) should be {3, 4, 5}
        for i in 3..=5 {
            assert!(preimage.contains(&i), "expected {i} in preimage of 1");
        }
    }
}

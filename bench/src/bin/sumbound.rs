// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! Nodes and latency of the search at budgets 32 and 48 on the M0 data, plus a
//! fingerprint of every hit list. Run the same binary against two builds of the
//! core (subtree bound combined with `max` and with a sum) and compare the
//! lines; see `docs/benchmarks/sum-bound.md`.
//!
//! Usage: `sumbound [DATA_DIR] [REPS]` (default `bench/data`, 5 timed passes).

use std::fs;
use std::time::Instant;

use keyhammer::cost::CostModel;
use keyhammer::search::{SearchConfig, Searcher};
use keyhammer::trie::Trie;

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(2);
}

fn read_tsv(path: &str) -> Vec<(String, String)> {
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| die(&format!("cannot read {path}: {e}")));
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|l| match l.split_once('\t') {
            Some((a, b)) => (a.to_string(), b.to_string()),
            None => die(&format!("{path}: line without a tab: {l:?}")),
        })
        .collect()
}

fn percentile(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[((v.len() as f64 * p) as usize).min(v.len() - 1)]
}

/// FNV-1a over (id, cost) of every hit of every query, in order.
fn fnv(h: &mut u64, x: u64) {
    for b in x.to_le_bytes() {
        *h = (*h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().unwrap_or_else(|| "bench/data".to_string());
    let reps: usize = args
        .next()
        .map_or(5, |s| s.parse().unwrap_or_else(|_| die("REPS")));
    let tests = read_tsv(&format!("{dir}/tests.tsv"));
    let cm = CostModel::qwerty();
    for size in ["10000", "100000", "full"] {
        let words: Vec<(String, u16)> = read_tsv(&format!("{dir}/words-{size}.tsv"))
            .into_iter()
            .map(|(w, f)| {
                (
                    w,
                    f.parse().unwrap_or_else(|_| die("frequency is not a u16")),
                )
            })
            .collect();
        let items: Vec<(&str, u16)> = words.iter().map(|(w, f)| (w.as_str(), *f)).collect();
        let trie = Trie::build(&items).unwrap_or_else(|_| die("trie"));
        for budget in [32u16, 48] {
            let cfg = SearchConfig {
                k: 10,
                budget,
                ..SearchConfig::default()
            };
            let mut s = Searcher::new();
            let (mut expanded, mut pushed, mut truncated, mut hash) =
                (0usize, 0usize, 0usize, 0xcbf2_9ce4_8422_2325u64);
            for (typo, _) in &tests {
                let out = s
                    .search(&trie, &cm, typo.as_bytes(), &cfg)
                    .unwrap_or_else(|_| die("search"));
                expanded += out.stats.nodes_expanded;
                pushed += out.stats.nodes_pushed;
                truncated += usize::from(out.stats.truncated);
                for h in &out.hits {
                    fnv(&mut hash, u64::from(h.id));
                    fnv(&mut hash, u64::from(h.cost));
                }
            }
            // timed passes; the p50/p95 of each pass, then the median over passes
            let (mut p50s, mut p95s) = (Vec::new(), Vec::new());
            for _ in 0..reps {
                let mut lat = Vec::new();
                for (typo, _) in &tests {
                    let t = Instant::now();
                    let out = s
                        .search(&trie, &cm, typo.as_bytes(), &cfg)
                        .unwrap_or_else(|_| die("search"));
                    lat.push(t.elapsed().as_secs_f64() * 1e6);
                    std::hint::black_box(out);
                }
                p50s.push(percentile(&mut lat.clone(), 0.5));
                p95s.push(percentile(&mut lat, 0.95));
            }
            let n = tests.len() as f64;
            println!(
                "{size:>6} words={:>6} budget={budget} nodes/query={:.1} pushed/query={:.1} truncated={truncated} hits_hash={hash:016x} p50_us={:.1} p95_us={:.1} p95_passes={:?}",
                words.len(),
                expanded as f64 / n,
                pushed as f64 / n,
                percentile(&mut p50s, 0.5),
                percentile(&mut p95s.clone(), 0.5),
                p95s.iter()
                    .map(|x| (x * 10.0).round() / 10.0)
                    .collect::<Vec<_>>()
            );
        }
    }
}

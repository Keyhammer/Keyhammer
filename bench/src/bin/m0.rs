// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! M0 gate harness: quality (MRR, R@1) and latency of the new engine, the legacy engine
//! and a plain "edit distance + frequency" baseline on the same typo pairs.

use std::fs;
use std::time::Instant;

use keyhammer::cost::CostModel;
use keyhammer::search::{SearchConfig, Searcher};
use keyhammer::trie::Trie;
use keyhammer_legacy::FuzzyIndex;

const TOP: usize = 10;

fn read_tsv(path: &str) -> Vec<(String, String)> {
    fs::read_to_string(path)
        .unwrap_or_else(|e| {
            panic!("cannot read {path}: {e} (run fetch-data.mjs and prepare-m0-data.mjs first)")
        })
        .lines()
        .filter_map(|l| {
            l.split_once('\t')
                .map(|(a, b)| (a.to_string(), b.to_string()))
        })
        .collect()
}

fn percentile(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[((v.len() as f64 * p) as usize).min(v.len() - 1)]
}

struct Row {
    name: &'static str,
    mrr: f64,
    r1: f64,
    p50_us: f64,
    p95_us: f64,
    extra: String,
}

fn score(rank: Option<usize>, mrr: &mut f64, r1: &mut usize) {
    if let Some(r) = rank {
        *mrr += 1.0 / (r as f64 + 1.0);
        if r == 0 {
            *r1 += 1;
        }
    }
}

fn osa_within(a: &[u8], b: &[u8], k: usize) -> Option<usize> {
    if a.len().abs_diff(b.len()) > k {
        return None;
    }
    let (m, n) = (a.len(), b.len());
    let mut d = vec![vec![0usize; n + 1]; m + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            let c = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + c);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    (d[m][n] <= k).then_some(d[m][n])
}

fn run(size: &str, dir: &str, tests: &[(String, String)]) -> Vec<Row> {
    let words: Vec<(String, u16)> = read_tsv(&format!("{dir}/words-{size}.tsv"))
        .into_iter()
        .map(|(w, f)| (w, f.parse().unwrap_or(0)))
        .collect();
    let mut rows = Vec::new();
    println!("\n=== dictionary: {} words ({size}) ===", words.len());

    // new engine, with and without TSB
    let items: Vec<(&str, u16)> = words.iter().map(|(w, f)| (w.as_str(), *f)).collect();
    let t0 = Instant::now();
    let trie = Trie::build(&items).expect("trie");
    let build_ms = t0.elapsed().as_secs_f64() * 1e3;
    let cm = CostModel::qwerty();
    for (name, tsb) in [("new", false), ("new+tsb", true)] {
        let cfg = SearchConfig {
            k: TOP,
            tsb,
            ..SearchConfig::default()
        };
        let mut searcher = Searcher::new();
        let (mut mrr, mut r1, mut expanded) = (0.0, 0usize, 0usize);
        let mut lat = Vec::new();
        for (typo, right) in tests {
            let t = Instant::now();
            let out = searcher
                .search(&trie, &cm, typo.as_bytes(), &cfg)
                .expect("search");
            lat.push(t.elapsed().as_secs_f64() * 1e6);
            expanded += out.stats.nodes_expanded;
            score(
                out.hits.iter().position(|h| trie.term(h.id) == right),
                &mut mrr,
                &mut r1,
            );
        }
        let n = tests.len() as f64;
        rows.push(Row {
            name,
            mrr: mrr / n,
            r1: r1 as f64 / n,
            p50_us: percentile(&mut lat.clone(), 0.5),
            p95_us: percentile(&mut lat, 0.95),
            extra: format!(
                "nodes/query={:.0} build={build_ms:.0}ms",
                expanded as f64 / n
            ),
        });
    }

    // legacy engine
    let terms: Vec<&str> = words.iter().map(|(w, _)| w.as_str()).collect();
    let legacy = FuzzyIndex::build(&terms, 2).expect("legacy");
    let (mut mrr, mut r1) = (0.0, 0usize);
    let mut lat = Vec::new();
    for (typo, right) in tests {
        let t = Instant::now();
        let res = legacy.search(typo, TOP).unwrap_or_default();
        lat.push(t.elapsed().as_secs_f64() * 1e6);
        score(res.iter().position(|r| r.term == *right), &mut mrr, &mut r1);
    }
    let n = tests.len() as f64;
    rows.push(Row {
        name: "legacy",
        mrr: mrr / n,
        r1: r1 as f64 / n,
        p50_us: percentile(&mut lat.clone(), 0.5),
        p95_us: percentile(&mut lat, 0.95),
        extra: String::new(),
    });

    // baseline: unit edit distance <= 2, then weight, then id
    let (mut mrr, mut r1) = (0.0, 0usize);
    let mut lat = Vec::new();
    for (typo, right) in tests {
        let t = Instant::now();
        let mut cand: Vec<(usize, u32, usize)> = Vec::new();
        for (id, (w, f)) in words.iter().enumerate() {
            if let Some(d) = osa_within(typo.as_bytes(), w.as_bytes(), 2) {
                cand.push((d, 65_535 - u32::from(*f), id));
            }
        }
        cand.sort();
        cand.truncate(TOP);
        lat.push(t.elapsed().as_secs_f64() * 1e6);
        score(
            cand.iter().position(|c| words[c.2].0 == *right),
            &mut mrr,
            &mut r1,
        );
    }
    rows.push(Row {
        name: "baseline",
        mrr: mrr / n,
        r1: r1 as f64 / n,
        p50_us: percentile(&mut lat.clone(), 0.5),
        p95_us: percentile(&mut lat, 0.95),
        extra: String::new(),
    });

    for r in &rows {
        println!(
            "{:<9} MRR={:.3} R@1={:.3} p50={:>9.1}us p95={:>9.1}us {}",
            r.name, r.mrr, r.r1, r.p50_us, r.p95_us, r.extra
        );
    }
    rows
}

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "bench/data".to_string());
    let tests = read_tsv(&format!("{dir}/tests.tsv"));
    println!("{} typo pairs", tests.len());
    let mut last = Vec::new();
    for size in ["10000", "100000", "full"] {
        last = run(size, &dir, &tests);
    }
    let get = |n: &str| last.iter().find(|r| r.name == n);
    if let (Some(new), Some(tsb), Some(legacy), Some(base)) =
        (get("new"), get("new+tsb"), get("legacy"), get("baseline"))
    {
        let best = if tsb.p95_us < new.p95_us { tsb } else { new };
        let ratio = legacy.p95_us / best.p95_us;
        println!("\n=== G0 (largest dictionary) ===");
        println!(
            "p95 legacy / new = {ratio:.1}x (need >= 10x): {}",
            if ratio >= 10.0 { "PASS" } else { "FAIL" }
        );
        println!(
            "MRR new {:.3} vs baseline {:.3} (need >=): {}",
            best.mrr,
            base.mrr,
            if best.mrr >= base.mrr { "PASS" } else { "FAIL" }
        );
        println!("oracle equality: run `cargo test -p keyhammer` (must be green)");
    }
}

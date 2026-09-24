// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! Recall beyond two edits (issue #22): budgets 32 / 48 / 64 on Birkbeck (the 300
//! M0 pairs), the GitHub Typo Corpus sample and the Wikipedia sample, at 10 000,
//! 100 000 and 274 137 words. Prints, per corpus and dictionary: reachable pairs,
//! R@10, MRR@10, nodes per query and paired differences between budgets; then a
//! decomposition of the pairs that are unreachable at budget 32 (unit edit
//! distance, phonetic-key equality). With `--latency`, p50/p95 of one timed search
//! per query, median of three rounds (run this last, on a quiet machine).
//!
//! Run: `cargo run --release -p keyhammer-bench --bin recall -- bench/data [--latency]`
//! after `fetch-data.mjs`, `prepare-m0-data.mjs` and `fetch-typo-corpora.mjs`.

#[path = "../recall_common.rs"]
mod common;

use std::collections::HashMap;
use std::time::Instant;

use common::*;
use keyhammer::cost::CostModel;
use keyhammer::search::{Ranking, SearchConfig, Searcher};
use keyhammer::trie::Trie;

const BUDGETS: [u16; 3] = [32, 48, 64];
const SIZES: [(&str, usize); 3] = [
    ("10 000", 10_000),
    ("100 000", 100_000),
    ("274 137", usize::MAX),
];

struct Corpus {
    name: &'static str,
    pairs: Vec<(String, String)>,
}

fn cfg(budget: u16) -> SearchConfig {
    SearchConfig {
        k: 10,
        budget,
        tsb: true,
        ..SearchConfig::default()
    }
}

fn pct(x: usize, n: usize) -> f64 {
    100.0 * x as f64 / n as f64
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let latency = args.iter().any(|a| a == "--latency");
    let dir = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| "bench/data".into());
    let full = read_words(&format!("{dir}/words-full.tsv"));
    let corpora = vec![
        Corpus {
            name: "Birkbeck (300 M0 pairs)",
            pairs: read_pairs(&format!("{dir}/tests.tsv")),
        },
        Corpus {
            name: "GitHub Typo Corpus (1000 pairs)",
            pairs: read_pairs(&format!("{dir}/gtc.tsv")),
        },
        Corpus {
            name: "Wikipedia (1000 pairs)",
            pairs: read_pairs(&format!("{dir}/wiki.tsv")),
        },
    ];
    let cm = CostModel::qwerty();

    if latency {
        run_latency(&full, &corpora, &cm, &dir);
        return;
    }

    for c in &corpora {
        println!("\n## {} \n", c.name);
        let targets: Vec<&str> = c.pairs.iter().map(|p| p.1.as_str()).collect();
        for (label, size) in SIZES {
            let words = if c.name.starts_with("Birkbeck") && size != usize::MAX {
                let f = if size == 10_000 { "10000" } else { "100000" };
                read_words(&format!("{dir}/words-{f}.tsv"))
            } else {
                dictionary(&full, &targets, size)
            };
            let items: Vec<(&str, u16)> = words.iter().map(|(w, f)| (w.as_str(), *f)).collect();
            let trie = Trie::build(&items).expect("trie");
            let ids: HashMap<&str, u32> =
                (0..trie.len() as u32).map(|i| (trie.term(i), i)).collect();
            let right: Vec<u32> = c.pairs.iter().map(|p| ids[p.1.as_str()]).collect();
            println!("### {label} words (dictionary {} words)\n", words.len());
            println!(
                "| budget | reachable (exact cost <= budget) | R@10 | MRR@10 | nodes/query | truncated | dMRR vs previous (SE) | dR@10 vs previous (SE) |"
            );
            println!("|---|---|---|---|---|---|---|---|");

            // exact cost of the right word (None when above 64): one big search per query
            let mut s = Searcher::new();
            let big = SearchConfig {
                k: 500_000,
                budget: 64,
                max_nodes: 1_000_000_000,
                ranking: Ranking::Exact,
                tsb: true,
                ..SearchConfig::default()
            };
            let cost: Vec<Option<u16>> = c
                .pairs
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let out = s.search(&trie, &cm, p.0.as_bytes(), &big).expect("search");
                    assert!(out.hits.len() < 500_000 && !out.stats.truncated);
                    out.hits.iter().find(|h| h.id == right[i]).map(|h| h.cost)
                })
                .collect();

            let mut prev: Option<(Vec<f64>, Vec<f64>)> = None;
            for b in BUDGETS {
                let mut rr = Vec::new();
                let mut hit = Vec::new();
                let mut nodes = 0usize;
                let mut trunc = 0usize;
                for (i, p) in c.pairs.iter().enumerate() {
                    let out = s
                        .search(&trie, &cm, p.0.as_bytes(), &cfg(b))
                        .expect("search");
                    let idl: Vec<u32> = out.hits.iter().map(|h| h.id).collect();
                    let r = rr_of(&idl, right[i]);
                    rr.push(r);
                    hit.push(if r > 0.0 { 1.0 } else { 0.0 });
                    nodes += out.stats.nodes_expanded;
                    trunc += usize::from(out.stats.truncated);
                }
                let reach = cost.iter().filter(|x| x.is_some_and(|c| c <= b)).count();
                let (dm, dr) = match &prev {
                    Some((pr, ph)) => {
                        let (a, sa) = paired(&rr, pr);
                        let (h, sh) = paired(&hit, ph);
                        (format!("{a:+.4} ({sa:.4})"), format!("{h:+.4} ({sh:.4})"))
                    }
                    None => ("".into(), "".into()),
                };
                println!(
                    "| {b} | {reach} ({:.1}%) | {:.3} | {:.3} | {:.0} | {trunc} | {dm} | {dr} |",
                    pct(reach, c.pairs.len()),
                    mean(&hit),
                    mean(&rr),
                    nodes as f64 / c.pairs.len() as f64
                );
                prev = Some((rr, hit));
            }

            if size == usize::MAX {
                decompose(c, &cost);
            }
            println!();
        }
    }
}

/// Unreachable pairs at 32: by unit OSA distance, budget that reaches them, phonetic key.
fn decompose(c: &Corpus, cost: &[Option<u16>]) {
    let n = c.pairs.len();
    let un32: Vec<usize> = (0..n)
        .filter(|&i| !cost[i].is_some_and(|x| x <= 32))
        .collect();
    println!(
        "\nUnreachable at budget 32: {} of {n} ({:.1}%); at 48: {}; at 64: {}.\n",
        un32.len(),
        pct(un32.len(), n),
        (0..n)
            .filter(|&i| !cost[i].is_some_and(|x| x <= 48))
            .count(),
        (0..n)
            .filter(|&i| !cost[i].is_some_and(|x| x <= 64))
            .count()
    );
    println!(
        "| unit OSA distance typo-word | pairs | unreachable at 32 | of those reached at 48 | reached at 64 | still unreachable at 64 | same Metaphone key | same Soundex key |"
    );
    println!("|---|---|---|---|---|---|---|---|");
    for (lab, lo, hi) in [("1", 1, 1), ("2", 2, 2), ("3", 3, 3), ("4 or more", 4, 99)] {
        let all: Vec<usize> = (0..n)
            .filter(|&i| {
                let d = osa(c.pairs[i].0.as_bytes(), c.pairs[i].1.as_bytes());
                d >= lo && d <= hi
            })
            .collect();
        let u: Vec<usize> = all.iter().copied().filter(|i| un32.contains(i)).collect();
        let r48 = u
            .iter()
            .filter(|&&i| cost[i].is_some_and(|x| x <= 48))
            .count();
        let r64 = u
            .iter()
            .filter(|&&i| cost[i].is_some_and(|x| x <= 64))
            .count();
        let mp = u
            .iter()
            .filter(|&&i| metaphone(c.pairs[i].0.as_bytes()) == metaphone(c.pairs[i].1.as_bytes()))
            .count();
        let sx = u
            .iter()
            .filter(|&&i| soundex(c.pairs[i].0.as_bytes()) == soundex(c.pairs[i].1.as_bytes()))
            .count();
        println!(
            "| {lab} | {} | {} | {r48} | {r64} | {} | {mp} | {sx} |",
            all.len(),
            u.len(),
            u.len() - r64
        );
    }
    let un64: Vec<usize> = (0..n)
        .filter(|&i| !cost[i].is_some_and(|x| x <= 64))
        .collect();
    let mp = un64
        .iter()
        .filter(|&&i| metaphone(c.pairs[i].0.as_bytes()) == metaphone(c.pairs[i].1.as_bytes()))
        .count();
    println!(
        "\nOf the {} pairs still unreachable at 64, {mp} share the Metaphone-like key of the right word.",
        un64.len()
    );
    let mp_all = (0..n)
        .filter(|&i| metaphone(c.pairs[i].0.as_bytes()) == metaphone(c.pairs[i].1.as_bytes()))
        .count();
    println!("Over all {n} pairs, {mp_all} share it.");
}

fn run_latency(full: &[(String, u16)], corpora: &[Corpus], cm: &CostModel, dir: &str) {
    println!(
        "Latency: one timed search per query, no warm-up beyond one untimed pass, p50/p95 in microseconds, median of 3 rounds.\n"
    );
    println!("| corpus | words | budget | p50 (us) | p95 (us) | p95 / p95 at 32 | rounds p95 |");
    println!("|---|---|---|---|---|---|---|");
    for c in corpora {
        let targets: Vec<&str> = c.pairs.iter().map(|p| p.1.as_str()).collect();
        for (label, size) in SIZES {
            let words = if c.name.starts_with("Birkbeck") && size != usize::MAX {
                let f = if size == 10_000 { "10000" } else { "100000" };
                read_words(&format!("{dir}/words-{f}.tsv"))
            } else {
                dictionary(full, &targets, size)
            };
            let items: Vec<(&str, u16)> = words.iter().map(|(w, f)| (w.as_str(), *f)).collect();
            let trie = Trie::build(&items).expect("trie");
            let mut s = Searcher::new();
            let mut res: Vec<Vec<(f64, f64)>> = vec![Vec::new(); BUDGETS.len()];
            for _round in 0..3 {
                for (bi, &b) in BUDGETS.iter().enumerate() {
                    for p in &c.pairs {
                        let _ = s.search(&trie, cm, p.0.as_bytes(), &cfg(b));
                    }
                    let mut t: Vec<f64> = c
                        .pairs
                        .iter()
                        .map(|p| {
                            let t0 = Instant::now();
                            let out = s.search(&trie, cm, p.0.as_bytes(), &cfg(b));
                            let e = t0.elapsed().as_secs_f64() * 1e6;
                            std::hint::black_box(out).ok();
                            e
                        })
                        .collect();
                    t.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    let q = |f: f64| {
                        t[((t.len() as f64 * f).ceil() as usize)
                            .saturating_sub(1)
                            .min(t.len() - 1)]
                    };
                    res[bi].push((q(0.5), q(0.95)));
                }
            }
            let med = |v: &Vec<(f64, f64)>, k: usize| {
                let mut x: Vec<f64> = v.iter().map(|p| if k == 0 { p.0 } else { p.1 }).collect();
                x.sort_by(|a, b| a.partial_cmp(b).unwrap());
                x[x.len() / 2]
            };
            let base = med(&res[0], 1);
            for (bi, &b) in BUDGETS.iter().enumerate() {
                let rounds: Vec<String> = res[bi].iter().map(|p| format!("{:.0}", p.1)).collect();
                println!(
                    "| {} | {label} | {b} | {:.0} | {:.0} | {:.1}x | {} |",
                    c.name,
                    med(&res[bi], 0),
                    med(&res[bi], 1),
                    med(&res[bi], 1) / base,
                    rounds.join(" / ")
                );
            }
        }
    }
}

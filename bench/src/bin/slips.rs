// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! Finger-slip harness: ranking quality (MRR@10, R@1, R@10) of the engine with the
//! keyboard-aware costs (default budget 32 and the budget-48 preset) against the
//! unit-cost baseline "unit edit distance <= 2, then higher weight, then id" on the
//! finger-slip corpus built by `fetch-finger-slips.mjs`, overall and per category,
//! with paired standard errors. Quality only; latency is not measured here.

use std::collections::BTreeMap;
use std::fs;

use keyhammer::cost::CostModel;
use keyhammer::search::{SearchConfig, Searcher};
use keyhammer::trie::Trie;

const TOP: usize = 10;
const Z95: f64 = 1.96;
const CATS: [&str; 8] = [
    "sub_adjacent",
    "sub_other",
    "transposition",
    "extra_letter",
    "extra_doubled",
    "missing_letter",
    "missing_doubled",
    "two_edits",
];

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(2);
}

const HELP: &str = "\
Finger-slip harness: MRR@10, R@1 and R@10 of the engine (default, budget 48) and of a
unit-cost edit-distance baseline on the corpus from fetch-finger-slips.mjs.

USAGE:
    slips [DATA_DIR] [FILE]

ARGS:
    DATA_DIR    directory with words-full.tsv and the corpus (default: bench/data)
    FILE        corpus file inside DATA_DIR (default: slips-sample.tsv; slips.tsv is the
                whole corpus and makes the baseline slow, one full dictionary scan per pair)
";

struct Pair {
    typo: String,
    right: String,
    cat: String,
    first: bool,
    users: u32,
}

fn read_pairs(path: &str) -> Vec<Pair> {
    let text = fs::read_to_string(path).unwrap_or_else(|e| {
        die(&format!(
            "cannot read {path}: {e} (run fetch-finger-slips.mjs first)"
        ))
    });
    text.lines()
        .enumerate()
        .skip(1)
        .filter(|(_, l)| !l.is_empty())
        .map(|(i, l)| {
            let f: Vec<&str> = l.split('\t').collect();
            if f.len() != 6 {
                die(&format!(
                    "{path}:{}: expected 6 columns, got {}",
                    i + 1,
                    f.len()
                ));
            }
            Pair {
                typo: f[0].to_string(),
                right: f[1].to_string(),
                cat: f[2].to_string(),
                first: f[3] == "1",
                users: f[5]
                    .parse()
                    .unwrap_or_else(|_| die(&format!("{path}:{}: bad users {:?}", i + 1, f[5]))),
            }
        })
        .collect()
}

fn read_words(path: &str) -> Vec<(String, u16)> {
    let text = fs::read_to_string(path).unwrap_or_else(|e| {
        die(&format!(
            "cannot read {path}: {e} (run fetch-data.mjs and prepare-m0-data.mjs first)"
        ))
    });
    text.lines()
        .filter(|l| !l.is_empty())
        .enumerate()
        .map(
            |(i, l)| match l.split_once('\t').map(|(w, f)| (w, f.parse::<u16>())) {
                Some((w, Ok(f))) => (w.to_string(), f),
                _ => die(&format!("{path}:{}: expected `word TAB u16`", i + 1)),
            },
        )
        .collect()
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

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

/// Paired difference `a - b` over the selected indices: (mean, SE, n).
fn paired(a: &[f64], b: &[f64], idx: &[usize]) -> (f64, f64, usize) {
    let d: Vec<f64> = idx.iter().map(|&i| a[i] - b[i]).collect();
    let n = d.len();
    if n < 2 {
        return (mean(&d), f64::NAN, n);
    }
    let m = mean(&d);
    let var = d.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (n as f64 - 1.0);
    (m, (var / n as f64).sqrt(), n)
}

fn sig(m: f64, se: f64) -> &'static str {
    if se.is_nan() || se == 0.0 {
        "n/a"
    } else if (m / se).abs() >= Z95 {
        "yes"
    } else {
        "no"
    }
}

fn main() {
    let mut args = Vec::new();
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return;
            }
            _ if a.starts_with('-') => die(&format!("unknown option {a:?} (see --help)")),
            _ => args.push(a),
        }
    }
    if args.len() > 2 {
        die("expected at most DATA_DIR and FILE (see --help)");
    }
    let dir = args
        .first()
        .cloned()
        .unwrap_or_else(|| "bench/data".to_string());
    let file = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "slips-sample.tsv".to_string());
    let pairs = read_pairs(&format!("{dir}/{file}"));
    if pairs.is_empty() {
        die("no pairs");
    }
    let words = read_words(&format!("{dir}/words-full.tsv"));
    println!(
        "{} pairs from {file}, dictionary {} words",
        pairs.len(),
        words.len()
    );

    let items: Vec<(&str, u16)> = words.iter().map(|(w, f)| (w.as_str(), *f)).collect();
    let trie = Trie::build(&items).expect("trie");
    let cm = CostModel::qwerty();

    // reciprocal rank per pair for each system
    let mut systems: Vec<(&str, Vec<f64>)> = Vec::new();
    for (name, cfg) in [
        (
            "default (budget 32)",
            SearchConfig {
                k: TOP,
                tsb: true,
                ..SearchConfig::default()
            },
        ),
        (
            "high_recall (budget 48)",
            SearchConfig {
                k: TOP,
                ..SearchConfig::high_recall()
            },
        ),
    ] {
        let mut s = Searcher::new();
        let rr: Vec<f64> = pairs
            .iter()
            .map(|p| {
                let out = s
                    .search(&trie, &cm, p.typo.as_bytes(), &cfg)
                    .expect("search");
                out.hits
                    .iter()
                    .position(|h| trie.term(h.id) == p.right)
                    .map_or(0.0, |r| 1.0 / (r as f64 + 1.0))
            })
            .collect();
        systems.push((name, rr));
    }
    let rr: Vec<f64> = pairs
        .iter()
        .map(|p| {
            let mut cand: Vec<(usize, u32, usize)> = Vec::new();
            for (id, (w, f)) in words.iter().enumerate() {
                if let Some(d) = osa_within(p.typo.as_bytes(), w.as_bytes(), 2) {
                    cand.push((d, 65_535 - u32::from(*f), id));
                }
            }
            cand.sort();
            cand.truncate(TOP);
            cand.iter()
                .position(|c| words[c.2].0 == p.right)
                .map_or(0.0, |r| 1.0 / (r as f64 + 1.0))
        })
        .collect();
    systems.push(("unit-cost baseline", rr));

    let all: Vec<usize> = (0..pairs.len()).collect();
    let rare: Vec<usize> = all
        .iter()
        .copied()
        .filter(|&i| pairs[i].users <= 2)
        .collect();
    let stat = |rr: &[f64], idx: &[usize]| {
        let n = idx.len() as f64;
        let m = idx.iter().map(|&i| rr[i]).sum::<f64>() / n;
        let r1 = idx.iter().filter(|&&i| rr[i] == 1.0).count() as f64 / n;
        let r10 = idx.iter().filter(|&&i| rr[i] > 0.0).count() as f64 / n;
        (m, r1, r10)
    };

    for (label, idx) in [
        ("all pairs", &all),
        ("pairs typed by at most 2 participants", &rare),
    ] {
        println!("\n### {label} (n = {})\n", idx.len());
        println!("| system | MRR@10 | R@1 | R@10 |\n|---|---|---|---|");
        for (name, rr) in &systems {
            let (m, r1, r10) = stat(rr, idx);
            println!("| {name} | {m:.3} | {r1:.3} | {r10:.3} |");
        }
        println!(
            "\n| paired MRR difference | diff | SE | 95% interval | \\|z\\| >= 1.96 |\n|---|---|---|---|---|"
        );
        for (a, b) in [(0, 2), (1, 2), (1, 0)] {
            let (m, se, _) = paired(&systems[a].1, &systems[b].1, idx);
            println!(
                "| {} - {} | {m:+.4} | {se:.4} | [{:+.4}, {:+.4}] | {} |",
                systems[a].0,
                systems[b].0,
                m - Z95 * se,
                m + Z95 * se,
                sig(m, se)
            );
        }
    }

    println!("\n### per category (all pairs)\n");
    println!(
        "| category | n | MRR default | MRR budget 48 | MRR baseline | default - baseline (SE) | budget 48 - baseline (SE) |\n|---|---|---|---|---|---|---|"
    );
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, p) in pairs.iter().enumerate() {
        groups.entry(p.cat.clone()).or_default().push(i);
        groups
            .entry(if p.first {
                "first letter involved".into()
            } else {
                "first letter untouched".into()
            })
            .or_default()
            .push(i);
    }
    let mut order: Vec<String> = CATS.iter().map(|c| c.to_string()).collect();
    order.push("first letter involved".into());
    order.push("first letter untouched".into());
    for cat in order {
        let Some(idx) = groups.get(&cat) else {
            continue;
        };
        let m = |k: usize| stat(&systems[k].1, idx).0;
        let (d0, s0, _) = paired(&systems[0].1, &systems[2].1, idx);
        let (d1, s1, _) = paired(&systems[1].1, &systems[2].1, idx);
        println!(
            "| {cat} | {} | {:.3} | {:.3} | {:.3} | {d0:+.3} ({s0:.3}) | {d1:+.3} ({s1:.3}) |",
            idx.len(),
            m(0),
            m(1),
            m(2)
        );
    }
}

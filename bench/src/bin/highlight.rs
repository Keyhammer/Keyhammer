// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! Work of match highlighting (`Searcher::highlight`) on the M0 data, against
//! the work of the search that produced the hits.
//!
//! For each dictionary, the 300 Birkbeck misspellings (`tests.tsv`) are
//! searched with `SearchConfig::default()` in whole-term mode, and their first
//! 3 and 5 characters in prefix mode; every returned hit (at most 10 per
//! query) is then highlighted. Deterministic counters: hits, DP cells computed
//! by the highlighting (`Highlight::cells`: mean, 95th percentile and maximum
//! per hit, total per query), ranges per hit, and the DP cells of the search
//! (`rows_computed` times the row width `2 * budget / 8 + 1`). Every hit must
//! highlight with its own cost; a failure exits with status 1.
//!
//! With `--time`, it also times each part (search of all queries, then
//! highlighting of all their hits; best of five passes). Timings depend on the
//! machine and its load; the counters do not.
//!
//! USAGE: highlight [--time] [DATA_DIR]   (default bench/data; see fetch-data.mjs)

use std::fs;
use std::time::Instant;

use keyhammer::cost::CostModel;
use keyhammer::highlight::HighlightMode;
use keyhammer::search::{Hit, SearchConfig, Searcher};
use keyhammer::trie::Trie;

fn read_tsv(path: &str) -> Vec<(String, String)> {
    let text = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("error: cannot read {path}: {e}");
        std::process::exit(2);
    });
    text.lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| l.split_once('\t'))
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

/// Best of five wall-clock times of `f`, in microseconds.
fn best_of_five(mut f: impl FnMut()) -> f64 {
    (0..5)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed().as_secs_f64() * 1e6
        })
        .fold(f64::INFINITY, f64::min)
}

fn main() {
    let mut dir = "bench/data".to_string();
    let mut time = false;
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--time" => time = true,
            "-h" | "--help" => {
                println!("USAGE: highlight [--time] [DATA_DIR]");
                return;
            }
            _ => dir = a,
        }
    }
    let tests = read_tsv(&format!("{dir}/tests.tsv"));
    let cm = CostModel::qwerty();
    let cfg = SearchConfig::default();
    let row_width = usize::from(2 * cfg.budget / cm.c_indel_min() + 1);
    let mut failed = false;
    println!(
        "| words | mode | queries | hits | cells/hit mean | p95 | max | cells/query (highlight) | cells/query (search) | ranges/hit |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|");
    let mut timings = Vec::new();
    for size in ["10000", "100000", "full"] {
        let words: Vec<(String, u16)> = read_tsv(&format!("{dir}/words-{size}.tsv"))
            .into_iter()
            .map(|(w, f)| (w, f.parse().unwrap_or(0)))
            .collect();
        let items: Vec<(&str, u16)> = words.iter().map(|(w, f)| (w.as_str(), *f)).collect();
        let Ok(trie) = Trie::build(&items) else {
            eprintln!("error: cannot build the {size} dictionary");
            std::process::exit(2);
        };
        for (label, mode, cut) in [
            ("whole", HighlightMode::Whole, usize::MAX),
            ("prefix 3", HighlightMode::Prefix, 3),
            ("prefix 5", HighlightMode::Prefix, 5),
        ] {
            let queries: Vec<String> = tests
                .iter()
                .map(|(typo, _)| typo.chars().take(cut).collect())
                .collect();
            let mut s = Searcher::new();
            let search = |s: &mut Searcher, q: &str| match mode {
                HighlightMode::Prefix => s.search_prefix(&trie, &cm, q.as_bytes(), &cfg),
                _ => s.search(&trie, &cm, q.as_bytes(), &cfg),
            };
            let mut all: Vec<(usize, Vec<Hit>)> = Vec::new();
            let mut search_cells = 0usize;
            for (qi, q) in queries.iter().enumerate() {
                let out = search(&mut s, q).unwrap_or_else(|e| {
                    eprintln!("error: search {q:?}: {e}");
                    std::process::exit(2);
                });
                search_cells += out.stats.rows_computed * row_width;
                all.push((qi, out.hits));
            }
            let mut cells: Vec<usize> = Vec::new();
            let mut ranges = 0usize;
            for (qi, hits) in &all {
                let q = &queries[*qi];
                for hit in hits {
                    let term = trie.term(hit.id);
                    match s.highlight(&trie, &cm, q.as_bytes(), hit, term, mode) {
                        Ok(h) if h.cost == hit.cost => {
                            cells.push(h.cells);
                            ranges += h.ranges.len();
                        }
                        other => {
                            eprintln!("FAIL: {q:?} -> {term:?}: {other:?}");
                            failed = true;
                        }
                    }
                }
            }
            let n = cells.len().max(1);
            let total: usize = cells.iter().sum();
            cells.sort_unstable();
            let p95 = cells
                .get(((n as f64 * 0.95) as usize).min(n - 1))
                .copied()
                .unwrap_or(0);
            let max = cells.last().copied().unwrap_or(0);
            let nq = queries.len();
            println!(
                "| {} | {label} | {nq} | {} | {:.1} | {p95} | {max} | {:.0} | {:.0} | {:.2} |",
                trie.len(),
                cells.len(),
                total as f64 / n as f64,
                total as f64 / nq as f64,
                search_cells as f64 / nq as f64,
                ranges as f64 / n as f64,
            );
            if time {
                let t_search = best_of_five(|| {
                    for q in &queries {
                        let _ = search(&mut s, q);
                    }
                });
                let t_hl = best_of_five(|| {
                    for (qi, hits) in &all {
                        let q = &queries[*qi];
                        for hit in hits {
                            let _ =
                                s.highlight(&trie, &cm, q.as_bytes(), hit, trie.term(hit.id), mode);
                        }
                    }
                });
                let hits: usize = all.iter().map(|(_, h)| h.len()).sum();
                timings.push(format!(
                    "| {} | {label} | {:.1} | {:.2} | {:.1} |",
                    trie.len(),
                    t_search / nq as f64,
                    t_hl / hits.max(1) as f64,
                    t_hl / nq as f64,
                ));
            }
        }
    }
    if time {
        println!("\n| words | mode | search us/query | highlight us/hit | highlight us/query |");
        println!("|---|---|---|---|---|");
        for t in timings {
            println!("{t}");
        }
    }
    if failed {
        std::process::exit(1);
    }
}

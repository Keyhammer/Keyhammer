// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! Deterministic work counters of the prefix (autocomplete) mode against the
//! exact mode on the M0 dictionaries. No latency is measured.
//!
//! Queries are Birkbeck misspellings (`tests.tsv`, typo TAB intended word)
//! truncated to a prefix, so that the typo, if it is inside the kept part, is
//! still in the query. For each dictionary and prefix length it prints the mean
//! and 95th percentile of expanded trie nodes for `search` (whole-term match)
//! and `search_prefix` (`tsb` off and on), the DP rows computed, and how often the intended word is in
//! the top 10 of each.
//!
//! USAGE: prefix [DATA_DIR]   (default bench/data; see fetch-data.mjs)

use std::fs;

use keyhammer::cost::CostModel;
use keyhammer::search::{SearchConfig, Searcher};
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

#[derive(Default)]
struct Acc {
    nodes: Vec<usize>,
    pushed: Vec<usize>,
    rows: Vec<usize>,
    found: usize,
    truncated: usize,
}

impl Acc {
    fn report(&mut self, label: &str) {
        let n = self.nodes.len();
        self.nodes.sort_unstable();
        let mean = self.nodes.iter().sum::<usize>() as f64 / n as f64;
        let p95 = self.nodes[((n as f64 * 0.95) as usize).min(n - 1)];
        let pushed = self.pushed.iter().sum::<usize>() as f64 / n as f64;
        let rows = self.rows.iter().sum::<usize>() as f64 / n as f64;
        println!(
            "    {label:<22} expanded mean {mean:>8.1}  p95 {p95:>6}  rows mean {rows:>8.1}  pushed mean {pushed:>8.1}  in top 10 {:>5.1}%  truncated {}",
            100.0 * self.found as f64 / n as f64,
            self.truncated
        );
    }
}

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "bench/data".into());
    let pairs = read_tsv(&format!("{dir}/tests.tsv"));
    let cm = CostModel::qwerty();
    let mut s = Searcher::new();
    for size in ["10000", "100000", "full"] {
        let words = read_tsv(&format!("{dir}/words-{size}.tsv"));
        let items: Vec<(&str, u16)> = words
            .iter()
            .map(|(w, f)| (w.as_str(), f.parse().expect("weight is a u16")))
            .collect();
        let trie = Trie::build(&items).expect("dictionary builds");
        println!("dictionary {size}: {} words", trie.len());
        for cut in [3usize, 4, 5, 6] {
            let mut exact = Acc::default();
            let mut pre_off = Acc::default();
            let mut pre_on = Acc::default();
            for (typo, want) in &pairs {
                let q = &typo.as_bytes()[..cut.min(typo.len())];
                let base = SearchConfig::default();
                let cfg = |tsb| SearchConfig {
                    tsb,
                    ..base.clone()
                };
                let hit = |out: &keyhammer::search::Output| {
                    out.hits.iter().any(|h| trie.term(h.id) == want)
                };
                let o = s.search(&trie, &cm, q, &cfg(true)).expect("search");
                exact.nodes.push(o.stats.nodes_expanded);
                exact.pushed.push(o.stats.nodes_pushed);
                exact.rows.push(o.stats.rows_computed);
                exact.found += usize::from(hit(&o));
                exact.truncated += usize::from(o.stats.truncated);
                for (acc, tsb) in [(&mut pre_off, false), (&mut pre_on, true)] {
                    let o = s.search_prefix(&trie, &cm, q, &cfg(tsb)).expect("search");
                    acc.nodes.push(o.stats.nodes_expanded);
                    acc.pushed.push(o.stats.nodes_pushed);
                    acc.rows.push(o.stats.rows_computed);
                    acc.found += usize::from(hit(&o));
                    acc.truncated += usize::from(o.stats.truncated);
                }
            }
            println!(
                "  first {cut} letters of the typo ({} queries)",
                pairs.len()
            );
            exact.report("exact (tsb on)");
            pre_off.report("prefix (tsb off)");
            pre_on.report("prefix (tsb on)");
        }
    }
}

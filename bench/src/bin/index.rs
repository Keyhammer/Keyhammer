// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! Serialized index format (issue #26): file size against the in-memory trie,
//! determinism, validation (load) time, and search on the borrowed view
//! against the owned trie (identical hits and work counters; latency only as
//! an indication). See docs/benchmarks/index-format.md.
//!
//! Usage: index [DATA_DIR] [--latency]

use std::fs;
use std::hint::black_box;
use std::time::Instant;

use keyhammer::cost::CostModel;
use keyhammer::index::Index;
use keyhammer::search::{SearchConfig, Searcher};
use keyhammer::trie::Trie;

const LOAD_RUNS: usize = 21;

fn read_tsv(path: &str) -> Vec<(String, String)> {
    let text = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("error: cannot read {path}: {e}");
        std::process::exit(2);
    });
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let (a, b) = l.split_once('\t').expect("two tab-separated columns");
            (a.to_string(), b.to_string())
        })
        .collect()
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

fn percentile(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[((v.len() as f64 * p) as usize).min(v.len() - 1)]
}

/// Heap bytes of the in-memory trie, from its arrays: labels (1 byte each if
/// every label is at most U+00FF, else 4), the six other per-node arrays, the
/// per-term weight and input index, the term text and one 24-byte `String`
/// header per term (64-bit). Allocator overhead and spare capacity excluded.
fn trie_heap_bytes(t: &Trie) -> usize {
    let n = t.node_count();
    let wide = (0..n).any(|v| u32::from(t.label(v)) > 0xFF);
    let label = if wide { 4 } else { 1 };
    let per_node = label + 4 + 2 + 4 + 2 + 2 + 2 + 8;
    let text: usize = (0..t.len() as u32).map(|id| t.term(id).len()).sum();
    n * per_node + t.len() * (2 + 4 + 24) + text
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let latency = args.iter().any(|a| a == "--latency");
    let dir = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| "bench/data".to_string());
    let tests = read_tsv(&format!("{dir}/tests.tsv"));
    let cm = CostModel::qwerty();
    let cfg = SearchConfig::default();

    println!(
        "| Words | Nodes | File bytes | File bytes/term | In-memory heap (est.) | Heap bytes/term | File / heap |"
    );
    println!("| --- | --- | --- | --- | --- | --- | --- |");
    let mut rest = Vec::new();
    for size in ["10000", "100000", "full"] {
        let words: Vec<(String, u16)> = read_tsv(&format!("{dir}/words-{size}.tsv"))
            .into_iter()
            .map(|(w, f)| (w, f.parse().expect("u16 frequency")))
            .collect();
        let items: Vec<(&str, u16)> = words.iter().map(|(w, f)| (w.as_str(), *f)).collect();
        let t0 = Instant::now();
        let trie = Trie::build(&items).expect("trie");
        let build_ms = t0.elapsed().as_secs_f64() * 1e3;
        let bytes = trie.to_bytes().expect("to_bytes");
        assert_eq!(bytes, trie.to_bytes().unwrap(), "not deterministic");
        assert_eq!(
            bytes,
            Trie::build(&items).unwrap().to_bytes().unwrap(),
            "a second build gives other bytes"
        );
        let heap = trie_heap_bytes(&trie);
        let t = trie.len() as f64;
        println!(
            "| {} | {} | {} | {:.1} | {} | {:.1} | {:.2} |",
            trie.len(),
            trie.node_count(),
            bytes.len(),
            bytes.len() as f64 / t,
            heap,
            heap as f64 / t,
            bytes.len() as f64 / heap as f64
        );

        // A copy with its last byte flipped fails at the CRC, after reading
        // the header and checksumming the whole file: its load time is the
        // checksum's share of a load.
        let mut flipped = bytes.clone();
        *flipped.last_mut().unwrap() ^= 1;
        let mut crc = Vec::new();
        let mut load = Vec::new();
        let mut to_trie = Vec::new();
        let mut write = Vec::new();
        for _ in 0..LOAD_RUNS {
            let t0 = Instant::now();
            assert!(black_box(Index::from_bytes(black_box(&flipped))).is_err());
            crc.push(t0.elapsed().as_secs_f64() * 1e3);
            let t0 = Instant::now();
            let ix = black_box(Index::from_bytes(black_box(&bytes)).expect("load"));
            load.push(t0.elapsed().as_secs_f64() * 1e3);
            let t0 = Instant::now();
            black_box(ix.to_trie());
            to_trie.push(t0.elapsed().as_secs_f64() * 1e3);
            let t0 = Instant::now();
            black_box(trie.to_bytes().unwrap());
            write.push(t0.elapsed().as_secs_f64() * 1e3);
        }

        // Search: the view must give the same hits and work counters.
        let ix = Index::from_bytes(&bytes).unwrap();
        let mut s = Searcher::new();
        let (mut expanded, mut rows) = (0usize, 0usize);
        for (typo, _) in &tests {
            let q = typo.as_bytes();
            let a = s.search(&trie, &cm, q, &cfg).unwrap();
            let b = ix.search(&mut s, &cm, q, &cfg).unwrap();
            assert_eq!((&a.hits, &a.stats), (&b.hits, &b.stats), "q={typo}");
            let a = s.search_prefix(&trie, &cm, q, &cfg).unwrap();
            let b = ix.search_prefix(&mut s, &cm, q, &cfg).unwrap();
            assert_eq!((&a.hits, &a.stats), (&b.hits, &b.stats), "prefix q={typo}");
            expanded += b.stats.nodes_expanded;
            rows += b.stats.rows_computed;
        }
        let mut lat = String::new();
        if latency {
            let mut owned = Vec::new();
            let mut view = Vec::new();
            for round in 0..3 {
                for (typo, _) in &tests {
                    let q = typo.as_bytes();
                    // Alternate the order so neither side always runs warm.
                    let mut one = |on_view: bool| {
                        let t0 = Instant::now();
                        if on_view {
                            black_box(ix.search(&mut s, &cm, q, &cfg).unwrap());
                        } else {
                            black_box(s.search(&trie, &cm, q, &cfg).unwrap());
                        }
                        t0.elapsed().as_secs_f64() * 1e6
                    };
                    if round == 0 {
                        one(false);
                        one(true);
                        continue; // warm-up round
                    }
                    let (o, v) = if round % 2 == 1 {
                        (one(false), one(true))
                    } else {
                        let v = one(true);
                        (one(false), v)
                    };
                    owned.push(o);
                    view.push(v);
                }
            }
            lat = format!(
                " | {:.1} | {:.1} | {:.1} | {:.1}",
                percentile(&mut owned.clone(), 0.5),
                percentile(&mut view.clone(), 0.5),
                percentile(&mut owned, 0.95),
                percentile(&mut view, 0.95)
            );
        }
        rest.push(format!(
            "| {} | {:.1} | {:.2} | {:.2} | {:.2} | {:.2} | {} | {}{} |",
            trie.len(),
            build_ms,
            median(write),
            median(load),
            median(crc),
            median(to_trie),
            expanded,
            rows,
            lat
        ));
    }
    println!();
    if latency {
        println!(
            "| Words | Build ms | to_bytes ms | from_bytes ms (validate) | of which CRC ms | to_trie ms | view: nodes expanded (exact+prefix, 300 q) | view: rows | owned p50 us | view p50 us | owned p95 us | view p95 us |"
        );
        println!("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |");
    } else {
        println!(
            "| Words | Build ms | to_bytes ms | from_bytes ms (validate) | of which CRC ms | to_trie ms | view: nodes expanded (exact+prefix, 300 q) | view: rows |"
        );
        println!("| --- | --- | --- | --- | --- | --- | --- | --- |");
    }
    for r in rest {
        println!("{r}");
    }
}

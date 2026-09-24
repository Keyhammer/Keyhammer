// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
//! Properties shared by the cargo-fuzz targets (`crates/keyhammer/fuzz`) and
//! the deterministic loop in `tests/fuzz_like.rs`. Each takes raw bytes, so the
//! same code runs under libFuzzer and under a seeded generator.
//!
//! The including crate must declare `mod support;` (the oracle) next to
//! `mod fuzz_props;`.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use crate::support::oracle_topk;
use keyhammer::cost::CostModel;
use keyhammer::search::{Ranking, SearchConfig, Searcher};
use keyhammer::trie::Trie;

/// Sequential reader over the fuzz input; reads past the end return 0.
struct Cursor<'a>(&'a [u8]);

impl Cursor<'_> {
    fn u8(&mut self) -> u8 {
        match self.0.split_first() {
            Some((&b, rest)) => {
                self.0 = rest;
                b
            }
            None => 0,
        }
    }
    fn u16(&mut self) -> u16 {
        u16::from_le_bytes([self.u8(), self.u8()])
    }
}

fn ranking(b: u8) -> Ranking {
    if b & 1 == 0 {
        Ranking::Coarse
    } else {
        Ranking::Exact
    }
}

/// Splits `data` into terms at 0xFF bytes; terms are lossily decoded UTF-8.
fn split_terms(data: &[u8], max_terms: usize) -> Vec<String> {
    data.split(|&b| b == 0xFF)
        .take(max_terms)
        .map(|t| String::from_utf8_lossy(t).into_owned())
        .collect()
}

fn build(terms: &[String], weights: &mut Cursor<'_>) -> Option<Trie> {
    let items: Vec<(&str, u16)> = terms.iter().map(|t| (t.as_str(), weights.u16())).collect();
    Trie::build(&items).ok()
}

/// Layout: `[k, budget_lo, budget_hi, max_nodes(2), flags, qlen(2)]`, then the
/// query, then terms separated by 0xFF. The budget is unrestricted, so
/// over-large budgets must come back as `Err`, never a panic.
pub fn never_panics(data: &[u8]) {
    let mut c = Cursor(data);
    let k = usize::from(c.u8());
    let budget = c.u16();
    let max_nodes = usize::from(c.u16());
    let flags = c.u8();
    let qlen = usize::from(c.u16()).min(c.0.len());
    let (q, rest) = c.0.split_at(qlen);
    let terms = split_terms(rest, 64);
    let mut w = Cursor(&[]);
    let Some(trie) = build(&terms, &mut w) else {
        return;
    };
    let cfg = SearchConfig {
        k,
        budget,
        tsb: flags & 2 != 0,
        max_nodes,
        ranking: ranking(flags),
    };
    let cm = CostModel::qwerty();
    if let Ok(out) = Searcher::new().search(&trie, &cm, q, &cfg) {
        assert!(out.hits.len() <= k);
        assert!(out.hits.iter().all(|h| h.cost <= budget));
    }
}

/// Layout: `[flags, k, budget, qlen]`, then the query, then terms split at
/// 0xFF. Bytes are folded onto `a..=h` so that matches actually occur, sizes
/// are capped for a fast brute-force oracle, and the node limit is lifted so
/// the search is exact. Runs tsb off and on with both rankings.
pub fn oracle_equality(data: &[u8]) {
    let mut c = Cursor(data);
    let flags = c.u8();
    let k = 1 + usize::from(c.u8() % 12);
    let budget = u16::from(c.u8() % 65);
    let qlen = usize::from(c.u8() % 13).min(c.0.len());
    let fold = |b: &u8| b'a' + (b % 8);
    let (qraw, rest) = c.0.split_at(qlen);
    let q: Vec<u8> = qraw.iter().map(fold).collect();
    let terms: Vec<String> = rest
        .split(|&b| b == 0xFF)
        .take(24)
        .filter(|t| !t.is_empty())
        .map(|t| t.iter().take(10).map(fold).map(char::from).collect())
        .collect();
    let wbytes = [flags, k as u8, 7, 3];
    let mut w = Cursor(&wbytes);
    let Some(trie) = build(&terms, &mut w) else {
        return;
    };
    let cm = CostModel::qwerty();
    let mut s = Searcher::new();
    for rk in [Ranking::Coarse, Ranking::Exact] {
        let want = oracle_topk(&trie, &cm, &q, budget, k, rk);
        for tsb in [false, true] {
            let cfg = SearchConfig {
                k,
                budget,
                tsb,
                max_nodes: usize::MAX,
                ranking: rk,
            };
            let out = s.search(&trie, &cm, &q, &cfg).unwrap();
            let got: Vec<(u32, u16)> = out.hits.iter().map(|h| (h.id, h.cost)).collect();
            assert_eq!(
                got, want,
                "q={q:?} terms={terms:?} budget={budget} k={k} tsb={tsb} {rk:?}"
            );
        }
    }
}

/// Layout: `[flags, k, budget, qlen]`, then the query (raw bytes), then terms
/// split at 0xFF. Both searches must return identical hits and, absent
/// truncation, be untruncated together.
pub fn tsb_equivalence(data: &[u8]) {
    let mut c = Cursor(data);
    let flags = c.u8();
    let k = usize::from(c.u8() % 33);
    let budget = u16::from(c.u8() % 65);
    let qlen = usize::from(c.u8() % 129).min(c.0.len());
    let (q, rest) = c.0.split_at(qlen);
    let terms = split_terms(rest, 200);
    let wbytes = [flags, 9, 9, 9];
    let mut w = Cursor(&wbytes);
    let Some(trie) = build(&terms, &mut w) else {
        return;
    };
    let cm = CostModel::qwerty();
    let mut s = Searcher::new();
    let mut run = |tsb| {
        let cfg = SearchConfig {
            k,
            budget,
            tsb,
            max_nodes: usize::MAX,
            ranking: ranking(flags),
        };
        let out = s.search(&trie, &cm, q, &cfg).unwrap();
        out.hits.iter().map(|h| (h.id, h.cost)).collect::<Vec<_>>()
    };
    let off = run(false);
    let on = run(true);
    assert_eq!(off, on, "q={q:?} terms={terms:?} budget={budget} k={k}");
}

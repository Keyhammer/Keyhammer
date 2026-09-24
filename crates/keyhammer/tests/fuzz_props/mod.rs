// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
//! Properties shared by the cargo-fuzz targets (`crates/keyhammer/fuzz`) and
//! the deterministic loop in `tests/fuzz_like.rs`. Each takes raw bytes, so the
//! same code runs under libFuzzer and under a seeded generator.
//!
//! The including crate must declare `mod support;` (the oracle) next to
//! `mod fuzz_props;`.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use crate::support::{oracle_cost, oracle_topk};
use keyhammer::cost::{CostModel, Layout};
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

/// The layout picked by the top bits of the flags byte.
fn layout(b: u8) -> Layout {
    Layout::ALL[usize::from(b >> 5) % Layout::ALL.len()]
}

fn ranking(b: u8) -> Ranking {
    if b & 1 == 0 {
        Ranking::Coarse
    } else {
        Ranking::Exact
    }
}

/// Splits `data` at 0xFF into chunks; the first byte of a chunk is its weight
/// selector and the rest is the term. With `fold`, term bytes are mapped onto
/// `a..=h`, weights onto four values (so ties occur) and empty terms are
/// dropped; without it terms are lossily decoded UTF-8, weights span `u16` and
/// empty terms are kept, so `Trie::build` can return `Err`.
fn parse_terms(data: &[u8], max_terms: usize, fold: bool) -> Vec<(String, u16)> {
    data.split(|&b| b == 0xFF)
        .take(max_terms)
        .filter_map(|chunk| {
            let (&wb, t) = chunk.split_first().unwrap_or((&0, &[]));
            if fold {
                if t.is_empty() {
                    return None;
                }
                let term = t
                    .iter()
                    .take(10)
                    .map(|b| char::from(b'a' + b % 8))
                    .collect();
                Some((term, u16::from(wb % 4) * 100))
            } else {
                Some((String::from_utf8_lossy(t).into_owned(), u16::from(wb) * 257))
            }
        })
        .collect()
}

fn build(terms: &[(String, u16)]) -> Option<Trie> {
    let items: Vec<(&str, u16)> = terms.iter().map(|(t, w)| (t.as_str(), *w)).collect();
    Trie::build(&items).ok()
}

fn fold_query(q: &[u8]) -> Vec<u8> {
    q.iter().map(|b| b'a' + b % 8).collect()
}

/// Layout: `[k, budget(2), max_nodes(2), flags, qlen]`, then `qlen % 129`
/// query bytes, then chunks `[weight, term...]` separated by 0xFF. The budget
/// is unrestricted, so over-large budgets must come back as `Err`, never a
/// panic.
pub fn never_panics(data: &[u8]) {
    let mut c = Cursor(data);
    let k = usize::from(c.u8());
    let budget = c.u16();
    let max_nodes = usize::from(c.u16());
    let flags = c.u8();
    let qlen = (usize::from(c.u8()) % 129).min(c.0.len());
    let (q, rest) = c.0.split_at(qlen);
    let terms = parse_terms(rest, 64, false);
    let Some(trie) = build(&terms) else {
        return;
    };
    let cfg = SearchConfig {
        k,
        budget,
        tsb: flags & 2 != 0,
        max_nodes,
        ranking: ranking(flags),
    };
    let cm = CostModel::for_layout(layout(flags));
    if let Ok(out) = Searcher::new().search_prefix(&trie, &cm, q, &cfg) {
        assert!(out.hits.len() <= k);
        assert!(out.hits.iter().all(|h| h.cost <= budget));
    }
    if let Ok(out) = Searcher::new().search(&trie, &cm, q, &cfg) {
        assert!(out.hits.len() <= k);
        assert!(out.hits.iter().all(|h| h.cost <= budget));
    }
}

/// Layout: `[flags, k, budget, qlen]`, then `qlen % 17` query bytes, then
/// chunks `[weight, term...]` split at 0xFF. Bytes are folded onto `a..=h` so
/// that matches occur, sizes are capped for a fast oracle, and the search is
/// run exact (no node limit) and node-limited, tsb off and on, both rankings.
pub fn oracle_equality(data: &[u8]) {
    let mut c = Cursor(data);
    let flags = c.u8();
    let k = 1 + usize::from(c.u8() % 12);
    let budget = u16::from(c.u8() % 65);
    let qlen = (usize::from(c.u8()) % 17).min(c.0.len());
    let (qraw, rest) = c.0.split_at(qlen);
    let q = fold_query(qraw);
    let terms = parse_terms(rest, 24, true);
    let Some(trie) = build(&terms) else {
        return;
    };
    let cm = CostModel::for_layout(layout(flags));
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
            // A node limit may cut the search short; then whatever it returns
            // must still be true costs within the budget.
            let cfg = SearchConfig {
                max_nodes: 1 + usize::from(flags >> 2) % 40,
                ..cfg
            };
            let out = s.search(&trie, &cm, &q, &cfg).unwrap();
            let got: Vec<(u32, u16)> = out.hits.iter().map(|h| (h.id, h.cost)).collect();
            if out.stats.truncated {
                for h in &out.hits {
                    let t = trie.term(h.id).as_bytes();
                    assert_eq!(u32::from(h.cost), oracle_cost(&cm, &q, t));
                    assert!(h.cost <= budget);
                }
            } else {
                assert_eq!(
                    got, want,
                    "untruncated limited run, q={q:?} terms={terms:?}"
                );
            }
        }
    }
}

/// Layout as [`oracle_equality`] but with a `qlen % 17` query and up to 200
/// terms. Both searches (no node limit) must return identical hits.
pub fn tsb_equivalence(data: &[u8]) {
    let mut c = Cursor(data);
    let flags = c.u8();
    let k = usize::from(c.u8() % 33);
    let budget = u16::from(c.u8() % 65);
    let qlen = (usize::from(c.u8()) % 17).min(c.0.len());
    let (qraw, rest) = c.0.split_at(qlen);
    let q = fold_query(qraw);
    let terms = parse_terms(rest, 200, true);
    let Some(trie) = build(&terms) else {
        return;
    };
    let cm = CostModel::for_layout(layout(flags));
    let mut s = Searcher::new();
    let mut run = |tsb| {
        let cfg = SearchConfig {
            k,
            budget,
            tsb,
            max_nodes: usize::MAX,
            ranking: ranking(flags),
        };
        let out = s.search(&trie, &cm, &q, &cfg).unwrap();
        out.hits.iter().map(|h| (h.id, h.cost)).collect::<Vec<_>>()
    };
    let off = run(false);
    let on = run(true);
    assert_eq!(off, on, "q={q:?} terms={terms:?} budget={budget} k={k}");
}

/// Prefix mode. Layout as [`oracle_equality`]. `search_prefix` must equal the
/// brute-force prefix oracle (the smallest cost over all prefixes of a term),
/// for both rankings and `tsb` on and off; a node-limited run must return
/// true prefix costs within the budget in the exact order's first positions.
pub fn prefix_oracle_equality(data: &[u8]) {
    let mut c = Cursor(data);
    let flags = c.u8();
    let k = 1 + usize::from(c.u8() % 12);
    let budget = u16::from(c.u8() % 65);
    let qlen = (usize::from(c.u8()) % 17).min(c.0.len());
    let (qraw, rest) = c.0.split_at(qlen);
    let q = fold_query(qraw);
    let terms = parse_terms(rest, 24, true);
    let Some(trie) = build(&terms) else {
        return;
    };
    let cm = CostModel::qwerty();
    let mut s = Searcher::new();
    for rk in [Ranking::Coarse, Ranking::Exact] {
        let want = crate::support::oracle_topk_prefix(&trie, &cm, &q, budget, k, rk);
        for tsb in [false, true] {
            let cfg = SearchConfig {
                k,
                budget,
                tsb,
                max_nodes: usize::MAX,
                ranking: rk,
            };
            let out = s.search_prefix(&trie, &cm, &q, &cfg).unwrap();
            let got: Vec<(u32, u16)> = out.hits.iter().map(|h| (h.id, h.cost)).collect();
            assert_eq!(
                got, want,
                "q={q:?} terms={terms:?} budget={budget} k={k} tsb={tsb} {rk:?}"
            );
            let cfg = SearchConfig {
                max_nodes: 1 + usize::from(flags >> 2) % 40,
                ..cfg
            };
            let out = s.search_prefix(&trie, &cm, &q, &cfg).unwrap();
            let got: Vec<(u32, u16)> = out.hits.iter().map(|h| (h.id, h.cost)).collect();
            if out.stats.truncated {
                assert_eq!(
                    got[..],
                    want[..got.len()],
                    "truncated, q={q:?} terms={terms:?}"
                );
            } else {
                assert_eq!(
                    got, want,
                    "untruncated limited run, q={q:?} terms={terms:?}"
                );
            }
        }
    }
}

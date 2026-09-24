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
use keyhammer::text::{self, Normalizer, SourceMap};
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
/// panic. The same terms are also built with `build_normalized` and searched
/// with `search_text` and `search_prefix_text` (query lossily decoded).
pub fn never_panics(data: &[u8]) {
    let mut c = Cursor(data);
    let k = usize::from(c.u8());
    let budget = c.u16();
    let max_nodes = usize::from(c.u16());
    let flags = c.u8();
    let qlen = (usize::from(c.u8()) % 129).min(c.0.len());
    let (q, rest) = c.0.split_at(qlen);
    let terms = parse_terms(rest, 64, false);
    let cfg = SearchConfig {
        k,
        budget,
        tsb: flags & 2 != 0,
        max_nodes,
        ranking: ranking(flags),
    };
    let cm = CostModel::for_layout(layout(flags));
    let items: Vec<(&str, u16)> = terms.iter().map(|(t, w)| (t.as_str(), *w)).collect();
    let n = MODES[usize::from(flags >> 2) % MODES.len()];
    if let Ok(trie) = Trie::build_normalized(&items, &n) {
        let qt = String::from_utf8_lossy(q);
        let mut s = Searcher::new();
        for out in [
            s.search_text(&trie, &cm, &qt, &cfg),
            s.search_prefix_text(&trie, &cm, &qt, &cfg),
        ]
        .into_iter()
        .flatten()
        {
            assert!(out.hits.len() <= k);
            assert!(out.hits.iter().all(|h| h.cost <= budget));
        }
    }
    let Some(trie) = build(&terms) else {
        return;
    };
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

/// The four normaliser modes.
const MODES: [Normalizer; 4] = [
    Normalizer::new(),
    Normalizer::new().with_diacritic_folding(false),
    Normalizer::new().with_case_folding(false),
    Normalizer::new()
        .with_case_folding(false)
        .with_diacritic_folding(false),
];

/// Layout: `[a, b]` then any bytes, lossily decoded as UTF-8 (so any `&str`,
/// with U+FFFD for invalid sequences). In every mode, normalising must not
/// panic, must be deterministic and idempotent, and the source map must be
/// monotone, in range and convertible to byte and UTF-16 ranges of the
/// source; the output range `a..b` (clamped) must map to a valid range too.
pub fn normalize(data: &[u8]) {
    let mut c = Cursor(data);
    let (a, b) = (usize::from(c.u8()), usize::from(c.u8()));
    let s = String::from_utf8_lossy(c.0);
    let src_len = s.chars().count();
    for n in MODES {
        let out = n.normalize(&s);
        assert_eq!(n.normalize(&out), out, "not idempotent: {s:?} -> {out:?}");
        assert_eq!(n.normalize(&s), out);
        let mut map = SourceMap::new();
        assert_eq!(n.normalize_mapped(&s, &mut map), out);
        assert_eq!(map.len(), out.chars().count());
        assert_eq!(map.source_len(), src_len);
        let mut prev = 0;
        for i in 0..map.len() {
            let f = map.source_of(i).unwrap();
            assert!(f >= prev && f < src_len);
            prev = f;
        }
        let (a, b) = (a.min(map.len()), b.min(map.len()));
        let (a, b) = (a.min(b), a.max(b));
        for r in [0..map.len(), a..b] {
            let src = map.to_source(r).unwrap();
            assert!(src.start <= src.end && src.end <= src_len);
            let bytes = text::utf8_range(&s, src.clone()).unwrap();
            assert!(s.get(bytes).is_some());
            let units = text::utf16_range(&s, src).unwrap();
            assert!(units.end <= s.encode_utf16().count());
        }
    }
}

/// The alphabet of [`text_oracle_equality`]: case pairs, precomposed and
/// combining accents, the special foldings and two scripts.
const TEXT_ALPHABET: [char; 16] = [
    'a', 'A', 'á', 'e', 'é', 'É', '\u{301}', 'c', 'ç', 'Ç', 's', 'ß', 'æ', 'ж', 'Ж', '😀',
];

/// Layout as [`oracle_equality`], with the top bits of the flags byte also
/// choosing one of the four normaliser modes. Query and term bytes are mapped
/// onto [`TEXT_ALPHABET`]; the trie is built with `build_normalized` (a term
/// that normalises to nothing makes the build fail, which ends the case), and
/// `search_text` and `search_prefix_text` must equal the brute-force oracle
/// run on the normalised query, for both rankings and `tsb` on and off.
pub fn text_oracle_equality(data: &[u8]) {
    let mut c = Cursor(data);
    let flags = c.u8();
    let k = 1 + usize::from(c.u8() % 12);
    let budget = u16::from(c.u8() % 65);
    let qlen = (usize::from(c.u8()) % 17).min(c.0.len());
    let (qraw, rest) = c.0.split_at(qlen);
    let to_text = |b: &[u8]| -> String {
        b.iter()
            .map(|&x| TEXT_ALPHABET[usize::from(x % 16)])
            .collect()
    };
    let q = to_text(qraw);
    let terms: Vec<(String, u16)> = rest
        .split(|&b| b == 0xFF)
        .take(24)
        .filter_map(|chunk| {
            let (&wb, t) = chunk.split_first()?;
            let t = &t[..t.len().min(10)];
            (!t.is_empty()).then(|| (to_text(t), u16::from(wb % 4) * 100))
        })
        .collect();
    let n = MODES[usize::from(flags >> 3) % MODES.len()];
    let items: Vec<(&str, u16)> = terms.iter().map(|(t, w)| (t.as_str(), *w)).collect();
    let Ok(trie) = Trie::build_normalized(&items, &n) else {
        return;
    };
    let nq = n.normalize(&q);
    let cm = CostModel::for_layout(layout(flags));
    let mut s = Searcher::new();
    for rk in [Ranking::Coarse, Ranking::Exact] {
        let want = oracle_topk(&trie, &cm, nq.as_bytes(), budget, k, rk);
        let want_prefix =
            crate::support::oracle_topk_prefix(&trie, &cm, nq.as_bytes(), budget, k, rk);
        for tsb in [false, true] {
            let cfg = SearchConfig {
                k,
                budget,
                tsb,
                max_nodes: usize::MAX,
                ranking: rk,
            };
            let hits = |out: keyhammer::search::Output| -> Vec<(u32, u16)> {
                out.hits.iter().map(|h| (h.id, h.cost)).collect()
            };
            let got = hits(s.search_text(&trie, &cm, &q, &cfg).unwrap());
            assert_eq!(
                got, want,
                "q={q:?} terms={terms:?} {n:?} budget={budget} k={k}"
            );
            let got = hits(s.search_prefix_text(&trie, &cm, &q, &cfg).unwrap());
            assert_eq!(got, want_prefix, "prefix q={q:?} terms={terms:?} {n:?}");
        }
    }
}

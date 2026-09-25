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
use keyhammer::index::Index;
use keyhammer::search::{Output, Ranking, SearchConfig, Searcher};
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

/// The range invariants of `docs/design/highlighting.md` section 2.2: every
/// range and `aligned` are valid, agreeing ranges of `source` in the three
/// units; ranges are non-empty, sorted, not touching and inside `aligned`.
fn check_highlight(h: &keyhammer::highlight::Highlight, source: &str) {
    let n = source.chars().count();
    for r in h.ranges.iter().chain(std::iter::once(&h.aligned)) {
        assert!(r.chars.start <= r.chars.end && r.chars.end <= n);
        assert_eq!(
            text::utf8_range(source, r.chars.clone()),
            Some(r.utf8.clone())
        );
        assert_eq!(
            text::utf16_range(source, r.chars.clone()),
            Some(r.utf16.clone())
        );
    }
    for r in &h.ranges {
        assert!(r.chars.start < r.chars.end);
        assert!(h.aligned.chars.start <= r.chars.start && r.chars.end <= h.aligned.chars.end);
    }
    for w in h.ranges.windows(2) {
        assert!(w[0].chars.end < w[1].chars.start);
    }
}

/// Highlighting. Layout as [`text_oracle_equality`]. Every hit of
/// `search_text` and `search_prefix_text` must highlight in its mode with a
/// cost equal to `Hit::cost` and to the brute-force oracle, and well-formed
/// ranges. Then, on the raw bytes: a plain trie of the lossily decoded chunks
/// is highlighted with an arbitrary query, id, cost, mode and source string,
/// which must return `Ok` with well-formed ranges or an error, never panic.
pub fn highlight(data: &[u8]) {
    use keyhammer::highlight::HighlightMode;
    use keyhammer::search::Hit;
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
    let terms: Vec<String> = rest
        .split(|&b| b == 0xFF)
        .take(24)
        .filter_map(|chunk| {
            let t = chunk.get(1..).unwrap_or_default();
            let t = &t[..t.len().min(10)];
            (!t.is_empty()).then(|| to_text(t))
        })
        .collect();
    let n = MODES[usize::from(flags >> 3) % MODES.len()];
    let items: Vec<(&str, u16)> = terms.iter().map(|t| (t.as_str(), 1)).collect();
    let cm = CostModel::for_layout(layout(flags));
    let mut s = Searcher::new();
    if let Ok(trie) = Trie::build_normalized(&items, &n) {
        let nq = n.normalize(&q);
        let cfg = SearchConfig {
            k,
            budget,
            tsb: flags & 2 != 0,
            max_nodes: usize::MAX,
            ranking: ranking(flags),
        };
        for mode in [HighlightMode::Whole, HighlightMode::Prefix] {
            let out = match mode {
                HighlightMode::Whole => s.search_text(&trie, &cm, &q, &cfg),
                _ => s.search_prefix_text(&trie, &cm, &q, &cfg),
            }
            .unwrap();
            for hit in &out.hits {
                let source = items[trie.input_index(hit.id) as usize].0;
                let h = s
                    .highlight_text(&trie, &cm, &q, hit, source, mode)
                    .unwrap_or_else(|e| panic!("{e}: q={q:?} source={source:?} {n:?} {mode:?}"));
                let t = trie.term(hit.id).as_bytes();
                let want = match mode {
                    HighlightMode::Whole => oracle_cost(&cm, nq.as_bytes(), t),
                    _ => crate::support::oracle_prefix_cost(&cm, nq.as_bytes(), t),
                };
                assert_eq!(h.cost, hit.cost);
                assert_eq!(u32::from(h.cost), want, "q={q:?} source={source:?}");
                check_highlight(&h, source);
            }
        }
    }
    // Arbitrary inputs: never a panic.
    let raw_terms = parse_terms(rest, 16, false);
    let Some(trie) = build(&raw_terms) else {
        return;
    };
    let hit = Hit {
        id: u32::from(flags) % (trie.len() as u32 + 2),
        cost: budget,
        weight: 0,
    };
    let mode = if flags & 4 == 0 {
        HighlightMode::Whole
    } else {
        HighlightMode::Prefix
    };
    let source = trie.term(hit.id).to_owned();
    let other = String::from_utf8_lossy(qraw).into_owned();
    for src in [source.as_str(), other.as_str()] {
        if let Ok(h) = s.highlight(&trie, &cm, qraw, &hit, src, mode) {
            assert_eq!(h.cost, hit.cost);
            check_highlight(&h, src);
        }
    }
}

/// Bitwise reference CRC-32 (zlib), independent of the crate's table-driven
/// one.
pub fn crc32_reference(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

/// Recomputes the CRC field (bytes 24..28) of an index, so that a mutation
/// reaches the structural checks.
pub fn fix_crc(bytes: &mut [u8]) {
    if bytes.len() >= 28 {
        bytes[24..28].fill(0);
        let crc = crc32_reference(bytes);
        bytes[24..28].copy_from_slice(&crc.to_le_bytes());
    }
}

/// What an accepted index must satisfy (`docs/design/index-format.md`,
/// sections 5.4 and 6): the view, the owned trie loaded from it and a trie
/// rebuilt from its own terms, weights and normaliser give the same results
/// for every query in `queries`, both modes, both rankings, `tsb` on and off,
/// plain and text search; and loading then writing gives back `bytes`.
pub fn check_accepted(bytes: &[u8], queries: &[&str], cm: &CostModel, k: usize, budget: u16) {
    let ix = Index::from_bytes(bytes).expect("accepted");
    let owned = ix.to_trie();
    assert_eq!(
        owned.to_bytes().unwrap(),
        bytes,
        "load then write is not the identity"
    );
    let items: Vec<(&str, u16)> = (0..ix.len() as u32)
        .map(|id| (ix.term(id), ix.weight(id)))
        .collect();
    let rebuilt = match ix.normalizer() {
        Some(n) => Trie::build_normalized(&items, &n),
        None => Trie::build(&items),
    }
    .expect("the terms of an accepted index build");
    assert_eq!(rebuilt.len(), ix.len());
    assert_eq!(rebuilt.node_count(), ix.node_count());
    let mut s = Searcher::new();
    let key = |o: Output| (o.hits, o.stats);
    for q in queries {
        for ranking in [Ranking::Coarse, Ranking::Exact] {
            for tsb in [false, true] {
                let cfg = SearchConfig {
                    k,
                    budget,
                    tsb,
                    ranking,
                    ..SearchConfig::default()
                };
                let b = q.as_bytes();
                let want = key(s.search(&rebuilt, cm, b, &cfg).unwrap());
                assert_eq!(
                    key(ix.search(&mut s, cm, b, &cfg).unwrap()),
                    want,
                    "q={q:?}"
                );
                assert_eq!(key(s.search(&owned, cm, b, &cfg).unwrap()), want);
                let want = key(s.search_prefix(&rebuilt, cm, b, &cfg).unwrap());
                assert_eq!(key(ix.search_prefix(&mut s, cm, b, &cfg).unwrap()), want);
                assert_eq!(key(s.search_prefix(&owned, cm, b, &cfg).unwrap()), want);
                let want = key(s.search_text(&rebuilt, cm, q, &cfg).unwrap());
                assert_eq!(key(ix.search_text(&mut s, cm, q, &cfg).unwrap()), want);
                let want = key(s.search_prefix_text(&rebuilt, cm, q, &cfg).unwrap());
                assert_eq!(
                    key(ix.search_prefix_text(&mut s, cm, q, &cfg).unwrap()),
                    want
                );
            }
        }
    }
}

/// Layout: `[mode, k, budget, qlen]`, `qlen % 17` query bytes (mapped onto
/// [`TEXT_ALPHABET`]), then either
///
/// - `mode % 4 == 0`: the rest is fed to `Index::from_bytes` as it is; or
/// - otherwise: a length byte `L`, `L` bytes of `[weight, term...]` chunks
///   split at 0xFF (terms mapped onto [`TEXT_ALPHABET`], built plain or with
///   one of the four normalisers), written with `to_bytes`; then the rest,
///   read as `[pos lo, pos hi, xor]` triples, is XORed into the bytes, and with
///   `mode & 2` the CRC is recomputed so that the structural checks are hit.
///
/// `from_bytes` must never panic; an accepted index must pass
/// [`check_accepted`]. An unmutated index must be accepted.
pub fn index_from_bytes(data: &[u8]) {
    let mut c = Cursor(data);
    let mode = c.u8();
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
    let cm = CostModel::for_layout(layout(mode));
    if mode % 4 == 0 {
        if Index::from_bytes(rest).is_ok() {
            check_accepted(rest, &[q.as_str(), ""], &cm, k, budget);
        }
        return;
    }
    let mut c = Cursor(rest);
    let len = usize::from(c.u8()).min(c.0.len());
    let (dict, muts) = c.0.split_at(len);
    let terms: Vec<(String, u16)> = dict
        .split(|&b| b == 0xFF)
        .take(16)
        .filter_map(|chunk| {
            let (&wb, t) = chunk.split_first()?;
            let t = &t[..t.len().min(8)];
            (!t.is_empty()).then(|| (to_text(t), u16::from(wb) * 257))
        })
        .collect();
    let items: Vec<(&str, u16)> = terms.iter().map(|(t, w)| (t.as_str(), *w)).collect();
    let built = if mode & 4 != 0 {
        Trie::build(&items)
    } else {
        Trie::build_normalized(&items, &MODES[usize::from(mode >> 3) % MODES.len()])
    };
    let Ok(trie) = built else {
        return;
    };
    let mut bytes = trie.to_bytes().unwrap();
    assert!(
        Index::from_bytes(&bytes).is_ok(),
        "a written index must load"
    );
    let mut changed = false;
    for m in muts.chunks_exact(3) {
        let pos = usize::from(u16::from_le_bytes([m[0], m[1]])) % bytes.len();
        bytes[pos] ^= m[2];
        changed |= m[2] != 0;
    }
    if mode & 2 != 0 {
        fix_crc(&mut bytes);
    }
    match Index::from_bytes(&bytes) {
        Ok(_) => check_accepted(&bytes, &[q.as_str(), "", "a"], &cm, k, budget),
        Err(e) => {
            assert!(changed, "an unmutated index was rejected: {e}");
            assert!(!e.to_string().is_empty());
        }
    }
}

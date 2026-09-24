// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
//! Differential tests of the trie search against a brute-force oracle.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use keyhammer::cost::{CostModel, Layout};
use keyhammer::search::{Ranking, SearchConfig, Searcher};
use keyhammer::text::Normalizer;
use keyhammer::trie::Trie;
use support::{Rng, oracle_topk, oracle_topk_prefix};

fn random_word(rng: &mut Rng, alpha: u64) -> Vec<u8> {
    let len = 1 + rng.below(9) as usize;
    (0..len).map(|_| b'a' + rng.below(alpha) as u8).collect()
}

fn mutate(rng: &mut Rng, word: &[u8], alpha: u64, ops: usize) -> Vec<u8> {
    let mut w = word.to_vec();
    for _ in 0..ops {
        match rng.below(4) {
            0 if !w.is_empty() => {
                let i = rng.below(w.len() as u64) as usize;
                w[i] = b'a' + rng.below(alpha) as u8;
            }
            1 => {
                let i = rng.below(w.len() as u64 + 1) as usize;
                w.insert(i, b'a' + rng.below(alpha) as u8);
            }
            2 if w.len() > 1 => {
                let i = rng.below(w.len() as u64) as usize;
                w.remove(i);
            }
            3 if w.len() > 1 => {
                let i = rng.below(w.len() as u64 - 1) as usize;
                w.swap(i, i + 1);
            }
            _ => {}
        }
    }
    w
}

fn check(seed: u64, alpha: u64, dict_size: usize, tsb: bool, ranking: Ranking) {
    check_with(Layout::Qwerty, seed, alpha, dict_size, tsb, ranking);
}

fn check_with(
    layout: Layout,
    seed: u64,
    alpha: u64,
    dict_size: usize,
    tsb: bool,
    ranking: Ranking,
) {
    let mut rng = Rng::new(seed);
    let words: Vec<Vec<u8>> = (0..dict_size)
        .map(|_| random_word(&mut rng, alpha))
        .collect();
    let weights: Vec<u16> = (0..dict_size)
        .map(|_| {
            if rng.below(3) == 0 {
                100
            } else {
                rng.below(65_536) as u16
            }
        })
        .collect();
    let strings: Vec<String> = words
        .iter()
        .map(|w| String::from_utf8(w.clone()).unwrap())
        .collect();
    let items: Vec<(&str, u16)> = strings
        .iter()
        .map(String::as_str)
        .zip(weights.iter().copied())
        .collect();
    let trie = Trie::build(&items).unwrap();
    let cm = CostModel::for_layout(layout);
    let mut searcher = Searcher::new();

    for _ in 0..60 {
        let q = if rng.below(5) == 0 {
            random_word(&mut rng, alpha)
        } else {
            let base = &words[rng.below(dict_size as u64) as usize];
            let ops = rng.below(4) as usize;
            mutate(&mut rng, base, alpha, ops)
        };
        // Budgets 7 and 64 hit the band edges W = 0 and W = MAX_W (8).
        for (k, budget) in [(1usize, 16u16), (5, 32), (20, 24), (3, 7), (10, 64)] {
            let cfg = SearchConfig {
                k,
                budget,
                tsb,
                ranking,
                ..SearchConfig::default()
            };
            let got: Vec<(u32, u16)> = searcher
                .search(&trie, &cm, &q, &cfg)
                .unwrap()
                .hits
                .iter()
                .map(|h| (h.id, h.cost))
                .collect();
            let want = oracle_topk(&trie, &cm, &q, budget, k, ranking);
            assert_eq!(
                got,
                want,
                "layout={} seed={seed} alpha={alpha} q={:?} k={k} budget={budget} tsb={tsb} ranking={ranking:?}",
                layout.name(),
                String::from_utf8_lossy(&q)
            );
        }
    }
}

#[test]
fn matches_the_oracle_on_tiny_alphabets() {
    for seed in 0..25 {
        check(seed, 3, 120, false, Ranking::Coarse);
        if seed % 2 == 0 {
            check(seed, 3, 120, false, Ranking::Exact);
        }
    }
}

#[test]
fn matches_the_oracle_on_medium_alphabets() {
    for seed in 100..120 {
        check(seed, 8, 300, false, Ranking::Coarse);
        if seed % 2 == 0 {
            check(seed, 8, 300, false, Ranking::Exact);
        }
    }
}

#[test]
fn matches_the_oracle_on_the_full_alphabet() {
    for seed in 200..210 {
        check(seed, 26, 400, false, Ranking::Coarse);
        if seed % 2 == 0 {
            check(seed, 26, 400, false, Ranking::Exact);
        }
    }
}

#[test]
fn known_typos_rank_the_intended_word_first() {
    let cm = CostModel::qwerty();
    let trie = Trie::build(&[
        ("javascript", 10),
        ("typescript", 10),
        ("python", 10),
        ("rust", 10),
        ("java", 10),
    ])
    .unwrap();
    let mut s = Searcher::new();
    let cfg = SearchConfig::default();
    let first = |q: &[u8], s: &mut Searcher| {
        let h = s.search(&trie, &cm, q, &cfg).unwrap().hits;
        (trie.term(h[0].id).to_string(), h[0].cost)
    };
    assert_eq!(first(b"javasript", &mut s), ("javascript".to_string(), 16)); // skipped 'c'
    assert_eq!(first(b"pythno", &mut s), ("python".to_string(), 12)); // transposed "on"
    assert_eq!(first(b"ryst", &mut s), ("rust".to_string(), 8)); // 'y' typed for 'u': neighbouring keys
}

#[test]
fn tsb_matches_the_oracle_on_tiny_alphabets() {
    for seed in 300..325 {
        check(seed, 3, 120, true, Ranking::Coarse);
        if seed % 2 == 0 {
            check(seed, 3, 120, true, Ranking::Exact);
        }
    }
}

#[test]
fn tsb_matches_the_oracle_on_medium_alphabets() {
    for seed in 400..420 {
        check(seed, 8, 300, true, Ranking::Coarse);
        if seed % 2 == 0 {
            check(seed, 8, 300, true, Ranking::Exact);
        }
    }
}

#[test]
fn tsb_matches_the_oracle_on_the_full_alphabet() {
    for seed in 500..510 {
        check(seed, 26, 400, true, Ranking::Coarse);
        if seed % 2 == 0 {
            check(seed, 26, 400, true, Ranking::Exact);
        }
    }
}

#[test]
fn tsb_and_plain_bound_return_identical_hits_and_tsb_never_pushes_more_in_total() {
    let mut rng = Rng::new(9);
    let strings: Vec<String> = (0..2000)
        .map(|_| String::from_utf8(random_word(&mut rng, 26)).unwrap())
        .collect();
    let items: Vec<(&str, u16)> = strings
        .iter()
        .map(|s| (s.as_str(), rng.below(65_536) as u16))
        .collect();
    let trie = Trie::build(&items).unwrap();
    let cm = CostModel::qwerty();
    let mut s = Searcher::new();
    let (mut pushed_plain, mut pushed_tsb) = (0usize, 0usize);
    for _ in 0..200 {
        let q = random_word(&mut rng, 26);
        let plain = s
            .search(
                &trie,
                &cm,
                &q,
                &SearchConfig {
                    tsb: false,
                    ..SearchConfig::default()
                },
            )
            .unwrap();
        let fast = s
            .search(
                &trie,
                &cm,
                &q,
                &SearchConfig {
                    tsb: true,
                    ..SearchConfig::default()
                },
            )
            .unwrap();
        assert_eq!(plain.hits, fast.hits);
        pushed_plain += plain.stats.nodes_pushed;
        pushed_tsb += fast.stats.nodes_pushed;
    }
    assert!(
        pushed_tsb <= pushed_plain,
        "tsb pushed {pushed_tsb}, plain pushed {pushed_plain}"
    );
}

/// `SearchConfig::default()` turns the subtree bound on (issue #49); the
/// results are identical with it off, and turning it off is still possible.
#[test]
fn the_default_enables_the_bound_and_results_do_not_depend_on_it() {
    assert!(SearchConfig::default().tsb);
    assert!(SearchConfig::high_recall().tsb);
    let mut rng = Rng::new(49);
    let strings: Vec<String> = (0..1000)
        .map(|_| String::from_utf8(random_word(&mut rng, 26)).unwrap())
        .collect();
    let items: Vec<(&str, u16)> = strings
        .iter()
        .map(|s| (s.as_str(), rng.below(65_536) as u16))
        .collect();
    let trie = Trie::build(&items).unwrap();
    let cm = CostModel::qwerty();
    let mut s = Searcher::new();
    for _ in 0..100 {
        let q = random_word(&mut rng, 26);
        let default = s.search(&trie, &cm, &q, &SearchConfig::default()).unwrap();
        let off = SearchConfig {
            tsb: false,
            ..SearchConfig::default()
        };
        let plain = s.search(&trie, &cm, &q, &off).unwrap();
        assert_eq!(default.hits, plain.hits);
        assert!(default.stats.nodes_expanded <= plain.stats.nodes_expanded);
    }
}

#[test]
fn both_rankings_return_the_same_candidate_set() {
    let cm = CostModel::qwerty();
    let mut s = Searcher::new();
    for seed in 600..610 {
        let mut rng = Rng::new(seed);
        let alpha = [3, 8, 26][seed as usize % 3];
        let strings: Vec<String> = (0..200)
            .map(|_| String::from_utf8(random_word(&mut rng, alpha)).unwrap())
            .collect();
        let items: Vec<(&str, u16)> = strings
            .iter()
            .map(|w| (w.as_str(), rng.below(65_536) as u16))
            .collect();
        let trie = Trie::build(&items).unwrap();
        for _ in 0..20 {
            let q = random_word(&mut rng, alpha);
            let mut sets = Vec::new();
            for ranking in [Ranking::Coarse, Ranking::Exact] {
                let cfg = SearchConfig {
                    k: trie.len() + 1,
                    ranking,
                    ..SearchConfig::default()
                };
                let mut hits: Vec<(u32, u16)> = s
                    .search(&trie, &cm, &q, &cfg)
                    .unwrap()
                    .hits
                    .iter()
                    .map(|h| (h.id, h.cost))
                    .collect();
                hits.sort_unstable();
                sets.push(hits);
            }
            assert_eq!(
                sets[0],
                sets[1],
                "seed={seed} q={:?}",
                String::from_utf8_lossy(&q)
            );
        }
    }
}

/// The high-recall preset (budget 48) must stay exact: same hits as brute force,
/// with the subtree bound on and off and with both rankings.
#[test]
fn the_high_recall_preset_matches_the_oracle() {
    let cm = CostModel::qwerty();
    for (alpha, dict_size, seeds) in [
        (3u64, 120usize, 600..612u64),
        (8, 300, 700..708),
        (26, 400, 800..804),
    ] {
        for seed in seeds {
            let mut rng = Rng::new(seed);
            let strings: Vec<String> = (0..dict_size)
                .map(|_| String::from_utf8(random_word(&mut rng, alpha)).unwrap())
                .collect();
            let items: Vec<(&str, u16)> = strings
                .iter()
                .map(|s| (s.as_str(), rng.below(65_536) as u16))
                .collect();
            let trie = Trie::build(&items).unwrap();
            let mut searcher = Searcher::new();
            for _ in 0..40 {
                let base = strings[rng.below(dict_size as u64) as usize].as_bytes();
                let ops = rng.below(5) as usize;
                let q = mutate(&mut rng, base, alpha, ops);
                for tsb in [false, true] {
                    for ranking in [Ranking::Coarse, Ranking::Exact] {
                        let cfg = SearchConfig {
                            tsb,
                            ranking,
                            ..SearchConfig::high_recall()
                        };
                        let got: Vec<(u32, u16)> = searcher
                            .search(&trie, &cm, &q, &cfg)
                            .unwrap()
                            .hits
                            .iter()
                            .map(|h| (h.id, h.cost))
                            .collect();
                        let want = oracle_topk(&trie, &cm, &q, cfg.budget, cfg.k, ranking);
                        assert_eq!(
                            got,
                            want,
                            "seed={seed} q={:?} tsb={tsb} ranking={ranking:?}",
                            String::from_utf8_lossy(&q)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn every_layout_matches_the_oracle_with_and_without_the_bound() {
    // The layout changes which substitutions are cheap, never the minimum
    // costs the bounds and the band width use; the search must stay exact.
    for &layout in Layout::ALL {
        for seed in 300..306 {
            for tsb in [false, true] {
                check_with(layout, seed, 26, 300, tsb, Ranking::Coarse);
                check_with(layout, seed + 50, 8, 200, tsb, Ranking::Exact);
            }
        }
    }
}

/// Prefix mode against the brute-force prefix oracle: queries are truncated
/// dictionary words with 0-3 edits (the autocomplete case), sometimes
/// unrelated words or empty. Runs the exact search and a node-limited one.
fn check_prefix(seed: u64, alpha: u64, dict_size: usize, tsb: bool, ranking: Ranking) {
    let mut rng = Rng::new(seed);
    let words: Vec<Vec<u8>> = (0..dict_size)
        .map(|_| random_word(&mut rng, alpha))
        .collect();
    let strings: Vec<String> = words
        .iter()
        .map(|w| String::from_utf8(w.clone()).unwrap())
        .collect();
    let items: Vec<(&str, u16)> = strings
        .iter()
        .map(|s| {
            let w = if rng.below(3) == 0 {
                100
            } else {
                rng.below(65_536) as u16
            };
            (s.as_str(), w)
        })
        .collect();
    let trie = Trie::build(&items).unwrap();
    let cm = CostModel::qwerty();
    let mut searcher = Searcher::new();

    for _ in 0..60 {
        let q = match rng.below(8) {
            0 => random_word(&mut rng, alpha),
            1 => Vec::new(),
            _ => {
                let base = &words[rng.below(dict_size as u64) as usize];
                let cut = 1 + rng.below(base.len() as u64) as usize;
                let ops = rng.below(4) as usize;
                mutate(&mut rng, &base[..cut], alpha, ops)
            }
        };
        for (k, budget) in [
            (1usize, 16u16),
            (5, 32),
            (20, 24),
            (3, 7),
            (10, 64),
            (7, 48),
        ] {
            let cfg = SearchConfig {
                k,
                budget,
                tsb,
                ranking,
                ..SearchConfig::default()
            };
            let out = searcher.search_prefix(&trie, &cm, &q, &cfg).unwrap();
            let got: Vec<(u32, u16)> = out.hits.iter().map(|h| (h.id, h.cost)).collect();
            let want = oracle_topk_prefix(&trie, &cm, &q, budget, k, ranking);
            assert_eq!(
                got,
                want,
                "seed={seed} alpha={alpha} q={:?} k={k} budget={budget} tsb={tsb} ranking={ranking:?}",
                String::from_utf8_lossy(&q)
            );
            assert!(!out.stats.truncated);
            // A node limit may cut the list short, never reorder or corrupt it.
            let cut = SearchConfig {
                max_nodes: 1 + rng.below(30) as usize,
                ..cfg
            };
            let out = searcher.search_prefix(&trie, &cm, &q, &cut).unwrap();
            let got: Vec<(u32, u16)> = out.hits.iter().map(|h| (h.id, h.cost)).collect();
            assert!(out.stats.nodes_expanded <= cut.max_nodes);
            if out.stats.truncated {
                assert_eq!(got[..], want[..got.len()], "seed={seed} truncated");
            } else {
                assert_eq!(got, want, "seed={seed} untruncated limited run");
            }
        }
    }
}

#[test]
fn prefix_matches_the_oracle_on_tiny_alphabets() {
    for seed in 600..625 {
        for tsb in [false, true] {
            check_prefix(seed, 3, 120, tsb, Ranking::Coarse);
            check_prefix(seed, 3, 120, tsb, Ranking::Exact);
        }
    }
}

#[test]
fn prefix_matches_the_oracle_on_medium_alphabets() {
    for seed in 700..715 {
        for tsb in [false, true] {
            check_prefix(seed, 8, 300, tsb, Ranking::Coarse);
            check_prefix(seed, 8, 300, tsb, Ranking::Exact);
        }
    }
}

#[test]
fn prefix_matches_the_oracle_on_the_full_alphabet() {
    for seed in 800..808 {
        for tsb in [false, true] {
            check_prefix(seed, 26, 400, tsb, Ranking::Coarse);
            check_prefix(seed, 26, 400, tsb, Ranking::Exact);
        }
    }
}

#[test]
fn prefix_cost_never_exceeds_the_exact_cost() {
    // A whole-term match is one of the prefixes, so the prefix cost of a term
    // never exceeds its exact cost.
    let mut rng = Rng::new(77);
    let words: Vec<Vec<u8>> = (0..200).map(|_| random_word(&mut rng, 4)).collect();
    let strings: Vec<String> = words
        .iter()
        .map(|w| String::from_utf8(w.clone()).unwrap())
        .collect();
    let items: Vec<(&str, u16)> = strings.iter().map(|s| (s.as_str(), 5)).collect();
    let trie = Trie::build(&items).unwrap();
    let cm = CostModel::qwerty();
    let mut s = Searcher::new();
    let cfg = SearchConfig {
        k: 1000,
        budget: 32,
        ranking: Ranking::Exact,
        ..SearchConfig::default()
    };
    for w in words.iter().take(60) {
        let ex = s.search(&trie, &cm, w, &cfg).unwrap();
        let pre = s.search_prefix(&trie, &cm, w, &cfg).unwrap();
        for h in &ex.hits {
            let p = pre.hits.iter().find(|p| p.id == h.id).unwrap();
            assert!(p.cost <= h.cost);
        }
        assert!(pre.hits.len() >= ex.hits.len());
    }
}

// ---- non-ASCII alphabets (issue #19) ----------------------------------------

/// Alphabets of code points: Latin-1 letters (narrow labels); `à` and `Ć`,
/// whose subtree classes collide; other scripts and an emoji (wide labels);
/// `ç` with its ABNT2 neighbours; and a mix of precomposed and combining
/// accents, which the engine compares as they are.
const TEXT_ALPHABETS: [&str; 5] = ["aeéèçãß", "aàĆc", "жзaЖ😀", "çlp.;", "ae\u{301}éæœ"];

fn text_word(rng: &mut Rng, alpha: &[char]) -> Vec<char> {
    let len = 1 + rng.below(9) as usize;
    (0..len)
        .map(|_| alpha[rng.below(alpha.len() as u64) as usize])
        .collect()
}

fn text_mutate(rng: &mut Rng, word: &[char], alpha: &[char], ops: usize) -> Vec<char> {
    let mut w = word.to_vec();
    let pick = |rng: &mut Rng| alpha[rng.below(alpha.len() as u64) as usize];
    for _ in 0..ops {
        match rng.below(4) {
            0 if !w.is_empty() => {
                let i = rng.below(w.len() as u64) as usize;
                w[i] = pick(rng);
            }
            1 => {
                let i = rng.below(w.len() as u64 + 1) as usize;
                let c = pick(rng);
                w.insert(i, c);
            }
            2 if w.len() > 1 => {
                let i = rng.below(w.len() as u64) as usize;
                w.remove(i);
            }
            3 if w.len() > 1 => {
                let i = rng.below(w.len() as u64 - 1) as usize;
                w.swap(i, i + 1);
            }
            _ => {}
        }
    }
    w
}

/// Exact and prefix search against the brute-force oracle over code points,
/// on a dictionary built with `Trie::build` (no normalisation).
fn check_text(layout: Layout, seed: u64, alpha: &str, dict_size: usize) {
    let alpha: Vec<char> = alpha.chars().collect();
    let mut rng = Rng::new(seed);
    let words: Vec<String> = (0..dict_size)
        .map(|_| text_word(&mut rng, &alpha).into_iter().collect())
        .collect();
    let items: Vec<(&str, u16)> = words
        .iter()
        .map(|w| (w.as_str(), rng.below(4) as u16 * 100))
        .collect();
    let trie = Trie::build(&items).unwrap();
    let cm = CostModel::for_layout(layout);
    let mut searcher = Searcher::new();
    for _ in 0..30 {
        let q: String = if rng.below(5) == 0 {
            text_word(&mut rng, &alpha).into_iter().collect()
        } else {
            let base: Vec<char> = words[rng.below(dict_size as u64) as usize]
                .chars()
                .collect();
            let cut = 1 + rng.below(base.len() as u64) as usize;
            let base = if rng.below(2) == 0 {
                &base[..]
            } else {
                &base[..cut]
            };
            let ops = rng.below(4) as usize;
            text_mutate(&mut rng, base, &alpha, ops)
                .into_iter()
                .collect()
        };
        let q = q.as_bytes();
        for (k, budget) in [(1usize, 16u16), (5, 32), (20, 24), (3, 7), (10, 64)] {
            for tsb in [false, true] {
                for ranking in [Ranking::Coarse, Ranking::Exact] {
                    let cfg = SearchConfig {
                        k,
                        budget,
                        tsb,
                        ranking,
                        ..SearchConfig::default()
                    };
                    let hits = |out: keyhammer::search::Output| -> Vec<(u32, u16)> {
                        out.hits.iter().map(|h| (h.id, h.cost)).collect()
                    };
                    let got = hits(searcher.search(&trie, &cm, q, &cfg).unwrap());
                    let want = oracle_topk(&trie, &cm, q, budget, k, ranking);
                    let ctx = format!(
                        "layout={} seed={seed} q={:?} k={k} budget={budget} tsb={tsb} {ranking:?}",
                        layout.name(),
                        String::from_utf8_lossy(q)
                    );
                    assert_eq!(got, want, "exact {ctx}");
                    let got = hits(searcher.search_prefix(&trie, &cm, q, &cfg).unwrap());
                    let want = oracle_topk_prefix(&trie, &cm, q, budget, k, ranking);
                    assert_eq!(got, want, "prefix {ctx}");
                }
            }
        }
    }
}

#[test]
fn text_matches_the_oracle_on_non_ascii_alphabets() {
    for (a, alpha) in TEXT_ALPHABETS.iter().enumerate() {
        for seed in 0..4 {
            let layout = Layout::ALL[(a + seed as usize) % Layout::ALL.len()];
            check_text(layout, 900 + 10 * a as u64 + seed, alpha, 150);
        }
    }
}

/// A multi-byte character is one symbol: one substitution, not two edits,
/// and a query as long as the limit in code points is accepted even if it
/// is longer in bytes.
#[test]
fn a_code_point_is_one_symbol() {
    let trie = Trie::build(&[("café", 1), ("cafe", 1), ("жук", 1)]).unwrap();
    let cm = CostModel::qwerty();
    let mut s = Searcher::new();
    let cfg = SearchConfig {
        k: 5,
        ranking: Ranking::Exact,
        ..SearchConfig::default()
    };
    let out = s.search(&trie, &cm, "cafe".as_bytes(), &cfg).unwrap();
    let got: Vec<(&str, u16)> = out.hits.iter().map(|h| (trie.term(h.id), h.cost)).collect();
    assert_eq!(got, [("cafe", 0), ("café", 16)]);
    let out = s.search(&trie, &cm, "жек".as_bytes(), &cfg).unwrap();
    assert_eq!(out.hits.len(), 1);
    assert_eq!((trie.term(out.hits[0].id), out.hits[0].cost), ("жук", 16));
    // 128 two-byte characters: 256 bytes, 128 code points.
    let long = "é".repeat(128);
    assert!(s.search(&trie, &cm, long.as_bytes(), &cfg).is_ok());
    let longer = "é".repeat(129);
    assert_eq!(
        s.search(&trie, &cm, longer.as_bytes(), &cfg).unwrap_err(),
        keyhammer::search::SearchError::QueryTooLong { len: 129, max: 128 }
    );
}

// ---- normalised dictionaries ------------------------------------------------

/// Raw text with uppercase, precomposed and combining accents, ligatures and
/// the special foldings, so that many raw strings normalise to one term.
const RAW: &str = "aAáÁeEéÉ\u{301}cCçÇsßæÆœİıiIжЖ😀ﬁ";

fn normalizers() -> [Normalizer; 3] {
    [
        Normalizer::new(),
        Normalizer::new().with_diacritic_folding(false),
        Normalizer::new().with_case_folding(false),
    ]
}

/// `search_text` and `search_prefix_text` on a trie built with
/// `build_normalized` against the brute-force oracle run on the normalised
/// query and the normalised terms.
fn check_normalized(n: Normalizer, layout: Layout, seed: u64, dict_size: usize) {
    let alpha: Vec<char> = RAW.chars().collect();
    let mut rng = Rng::new(seed);
    let words: Vec<String> = (0..dict_size)
        .map(|_| {
            let mut w: String = text_word(&mut rng, &alpha).into_iter().collect();
            // A leading letter, so that no term normalises to nothing.
            w.insert(0, alpha[rng.below(8) as usize]);
            w
        })
        .collect();
    let items: Vec<(&str, u16)> = words
        .iter()
        .map(|w| (w.as_str(), rng.below(4) as u16 * 100))
        .collect();
    let trie = Trie::build_normalized(&items, &n).unwrap();
    assert_eq!(trie.normalizer(), Some(n));
    // Every term is the normalised text of the entry it was kept from, and
    // that entry has the highest weight among those with the same text.
    for id in 0..trie.len() as u32 {
        let kept = trie.input_index(id) as usize;
        assert_eq!(trie.term(id), n.normalize(words[kept].as_str()));
        for (i, w) in words.iter().enumerate() {
            if n.normalize(w) == trie.term(id) {
                assert!(items[i].1 < items[kept].1 || (items[i].1 == items[kept].1 && i >= kept));
            }
        }
    }
    let cm = CostModel::for_layout(layout);
    let mut searcher = Searcher::new();
    for _ in 0..25 {
        let q: String = if rng.below(4) == 0 {
            text_word(&mut rng, &alpha).into_iter().collect()
        } else {
            let base: Vec<char> = words[rng.below(dict_size as u64) as usize]
                .chars()
                .collect();
            let cut = 1 + rng.below(base.len() as u64) as usize;
            let ops = rng.below(3) as usize;
            text_mutate(&mut rng, &base[..cut], &alpha, ops)
                .into_iter()
                .collect()
        };
        let nq = n.normalize(&q);
        for (k, budget) in [(1usize, 16u16), (5, 32), (10, 64)] {
            for tsb in [false, true] {
                for ranking in [Ranking::Coarse, Ranking::Exact] {
                    let cfg = SearchConfig {
                        k,
                        budget,
                        tsb,
                        ranking,
                        ..SearchConfig::default()
                    };
                    let hits = |out: keyhammer::search::Output| -> Vec<(u32, u16)> {
                        out.hits.iter().map(|h| (h.id, h.cost)).collect()
                    };
                    let ctx = format!("{n:?} seed={seed} q={q:?} k={k} budget={budget} tsb={tsb}");
                    let got = hits(searcher.search_text(&trie, &cm, &q, &cfg).unwrap());
                    let want = oracle_topk(&trie, &cm, nq.as_bytes(), budget, k, ranking);
                    assert_eq!(got, want, "exact {ctx}");
                    let got = hits(searcher.search_prefix_text(&trie, &cm, &q, &cfg).unwrap());
                    let want = oracle_topk_prefix(&trie, &cm, nq.as_bytes(), budget, k, ranking);
                    assert_eq!(got, want, "prefix {ctx}");
                }
            }
        }
    }
}

#[test]
fn normalized_text_matches_the_oracle() {
    for (i, n) in normalizers().into_iter().enumerate() {
        for seed in 0..3 {
            let layout = Layout::ALL[(i * 3 + seed as usize) % Layout::ALL.len()];
            check_normalized(n, layout, 950 + 10 * i as u64 + seed, 150);
        }
    }
}

#[test]
fn normalized_search_ignores_case_and_accents() {
    let items = [
        ("São Paulo", 10),
        ("Sao Paulo", 3),
        ("naïve", 1),
        ("Straße", 5),
        ("Ærø", 2),
        ("ﬁnale", 4),
        ("İzmir", 6),
        ("Ürümqi", 1),
    ];
    let trie = Trie::build_normalized(&items, &Normalizer::new()).unwrap();
    assert_eq!(trie.len(), 7); // the two São Paulo spellings merged
    let cm = CostModel::qwerty();
    let mut s = Searcher::new();
    let cfg = SearchConfig::default();
    for (q, want) in [
        ("sao paulo", 0usize),
        ("SÃO PAULO", 0),
        ("sa\u{303}o paulo", 0),
        ("naive", 2),
        ("NAÏVE", 2),
        ("strasse", 3),
        ("STRASSE", 3),
        ("aero", 4),
        ("finale", 5),
        ("izmir", 6),
        ("urumqi", 7),
    ] {
        let out = s.search_text(&trie, &cm, q, &cfg).unwrap();
        let first = out.hits[0];
        assert_eq!(
            (trie.input_index(first.id) as usize, first.cost),
            (want, 0),
            "{q}"
        );
    }
    // Without the normaliser the same query does not match exactly.
    let raw = Trie::build(&items).unwrap();
    let out = s.search_text(&raw, &cm, "sao paulo", &cfg).unwrap();
    assert!(out.hits.iter().all(|h| h.cost > 0));
}

#[test]
fn case_only_normalisation_keeps_accents_and_uses_abnt2_c_cedilla() {
    let n = Normalizer::new().with_diacritic_folding(false);
    let trie = Trie::build_normalized(&[("Ação", 1), ("Acao", 1), ("Alão", 1)], &n).unwrap();
    assert_eq!(trie.len(), 3);
    let mut s = Searcher::new();
    let cfg = SearchConfig {
        ranking: Ranking::Exact,
        ..SearchConfig::default()
    };
    let cost = |s: &mut Searcher, layout: Layout, q: &str, term: &str| {
        let out = s
            .search_text(&trie, &CostModel::for_layout(layout), q, &cfg)
            .unwrap();
        out.hits
            .iter()
            .find(|h| trie.term(h.id) == term)
            .map(|h| h.cost)
    };
    assert_eq!(cost(&mut s, Layout::Abnt2, "AÇÃO", "ação"), Some(0));
    // l typed for ç: neighbouring keys on ABNT2 only.
    assert_eq!(cost(&mut s, Layout::Abnt2, "alão", "ação"), Some(8));
    assert_eq!(cost(&mut s, Layout::Qwerty, "alão", "ação"), Some(16));
    assert_eq!(cost(&mut s, Layout::Abnt2, "ação", "acao"), Some(32));
}

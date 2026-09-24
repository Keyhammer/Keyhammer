// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
//! Differential tests of the trie search against a brute-force oracle.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use keyhammer::cost::CostModel;
use keyhammer::search::{Ranking, SearchConfig, Searcher};
use keyhammer::trie::Trie;
use support::{Rng, oracle_topk};

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
    let cm = CostModel::qwerty();
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
                "seed={seed} alpha={alpha} q={:?} k={k} budget={budget} tsb={tsb} ranking={ranking:?}",
                String::from_utf8_lossy(&q)
            );
        }
    }
}

#[test]
fn matches_the_oracle_on_tiny_alphabets() {
    for seed in 0..25 {
        check(seed, 3, 120, false, Ranking::Edits);
        if seed % 2 == 0 {
            check(seed, 3, 120, false, Ranking::Cost);
        }
    }
}

#[test]
fn matches_the_oracle_on_medium_alphabets() {
    for seed in 100..120 {
        check(seed, 8, 300, false, Ranking::Edits);
        if seed % 2 == 0 {
            check(seed, 8, 300, false, Ranking::Cost);
        }
    }
}

#[test]
fn matches_the_oracle_on_the_full_alphabet() {
    for seed in 200..210 {
        check(seed, 26, 400, false, Ranking::Edits);
        if seed % 2 == 0 {
            check(seed, 26, 400, false, Ranking::Cost);
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
        check(seed, 3, 120, true, Ranking::Edits);
        if seed % 2 == 0 {
            check(seed, 3, 120, true, Ranking::Cost);
        }
    }
}

#[test]
fn tsb_matches_the_oracle_on_medium_alphabets() {
    for seed in 400..420 {
        check(seed, 8, 300, true, Ranking::Edits);
        if seed % 2 == 0 {
            check(seed, 8, 300, true, Ranking::Cost);
        }
    }
}

#[test]
fn tsb_matches_the_oracle_on_the_full_alphabet() {
    for seed in 500..510 {
        check(seed, 26, 400, true, Ranking::Edits);
        if seed % 2 == 0 {
            check(seed, 26, 400, true, Ranking::Cost);
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

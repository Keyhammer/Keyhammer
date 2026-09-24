// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
//! Edge-case tests for the trie search.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use keyhammer::cost::CostModel;
use keyhammer::search::{MAX_QUERY_LEN, SearchConfig, SearchError, Searcher};
use keyhammer::trie::Trie;
use support::oracle_topk;

fn run(trie: &Trie, q: &[u8], cfg: &SearchConfig) -> Vec<(u32, u16)> {
    let cm = CostModel::qwerty();
    Searcher::new()
        .search(trie, &cm, q, cfg)
        .unwrap()
        .hits
        .iter()
        .map(|h| (h.id, h.cost))
        .collect()
}

fn words() -> Trie {
    Trie::build(&[
        ("a", 1),
        ("ab", 2),
        ("abc", 3),
        ("b1", 4),
        ("c-d", 5),
        ("café", 6),
        ("hello", 7),
        ("help", 8),
    ])
    .unwrap()
}

#[test]
fn non_letter_and_multibyte_bytes_do_not_panic_and_match_the_oracle() {
    let trie = words();
    let cm = CostModel::qwerty();
    let cases: [&[u8]; 7] = [
        b"b1",
        b"c-d",
        b"c d",
        "cafe".as_bytes(),
        "café".as_bytes(),
        b"\xff\xfe",
        b"9",
    ];
    for q in cases {
        let cfg = SearchConfig::default();
        assert_eq!(
            run(&trie, q, &cfg),
            oracle_topk(&trie, &cm, q, cfg.budget, cfg.k),
            "q={q:?}"
        );
    }
}

#[test]
fn empty_query_matches_the_oracle() {
    let trie = words();
    let cm = CostModel::qwerty();
    let cfg = SearchConfig::default();
    assert_eq!(
        run(&trie, b"", &cfg),
        oracle_topk(&trie, &cm, b"", cfg.budget, cfg.k)
    );
}

#[test]
fn length_mismatch_beyond_the_band_returns_nothing() {
    let trie = Trie::build(&[("cat", 1), ("dog", 1)]).unwrap();
    assert!(run(&trie, b"aaaaaaaaaaaaaaaaaaaa", &SearchConfig::default()).is_empty());
}

#[test]
fn k_zero_returns_nothing() {
    let cfg = SearchConfig {
        k: 0,
        ..SearchConfig::default()
    };
    assert!(run(&words(), b"abc", &cfg).is_empty());
}

#[test]
fn k_larger_than_the_match_count_returns_every_match() {
    let trie = words();
    let cm = CostModel::qwerty();
    let cfg = SearchConfig {
        k: 1000,
        ..SearchConfig::default()
    };
    let got = run(&trie, b"hel", &cfg);
    assert_eq!(got, oracle_topk(&trie, &cm, b"hel", cfg.budget, 1000));
    assert!(got.len() < 1000);
}

#[test]
fn a_tiny_node_limit_truncates_and_returns_a_correct_prefix() {
    let trie = words();
    let cm = CostModel::qwerty();
    let full = oracle_topk(&trie, &cm, b"hel", 32, 10);
    assert!(full.len() >= 2, "the fixture needs at least two matches");
    let mut saw_partial = false;
    let mut saw_complete = false;
    for max_nodes in 1..=trie.node_count() + 1 {
        let cfg = SearchConfig {
            max_nodes,
            ..SearchConfig::default()
        };
        let out = Searcher::new().search(&trie, &cm, b"hel", &cfg).unwrap();
        let got: Vec<(u32, u16)> = out.hits.iter().map(|h| (h.id, h.cost)).collect();
        assert!(got.len() <= full.len(), "max_nodes={max_nodes}");
        assert_eq!(got, full[..got.len()].to_vec(), "max_nodes={max_nodes}");
        if got.len() < full.len() {
            assert!(out.stats.truncated, "max_nodes={max_nodes}");
        }
        saw_partial |= !got.is_empty() && got.len() < full.len();
        saw_complete |= got.len() == full.len() && !out.stats.truncated;
    }
    assert!(
        saw_partial,
        "no node limit produced a non-empty strict prefix"
    );
    assert!(saw_complete, "no node limit let the search complete");
}

#[test]
fn duplicate_terms_and_equal_weights_give_a_deterministic_order() {
    let trie = Trie::build(&[("bat", 5), ("cat", 5), ("hat", 5), ("cat", 9)]).unwrap();
    let cm = CostModel::qwerty();
    let cfg = SearchConfig::default();
    let a = run(&trie, b"xat", &cfg);
    let b = run(&trie, b"xat", &cfg);
    assert_eq!(a, b);
    assert_eq!(a, oracle_topk(&trie, &cm, b"xat", cfg.budget, cfg.k));
}

#[test]
fn oversized_query_and_budget_are_typed_errors() {
    let trie = words();
    let cm = CostModel::qwerty();
    let long = vec![b'a'; MAX_QUERY_LEN + 1];
    let e = Searcher::new()
        .search(&trie, &cm, &long, &SearchConfig::default())
        .unwrap_err();
    assert_eq!(
        e,
        SearchError::QueryTooLong {
            len: MAX_QUERY_LEN + 1,
            max: MAX_QUERY_LEN
        }
    );
    let cfg = SearchConfig {
        budget: 80,
        ..SearchConfig::default()
    };
    let e = Searcher::new().search(&trie, &cm, b"a", &cfg).unwrap_err();
    assert!(matches!(e, SearchError::BudgetTooLarge { budget: 80, .. }));
}

#[test]
fn the_reported_budget_limit_is_the_largest_accepted_budget() {
    let trie = words();
    let cm = CostModel::qwerty();
    let probe = SearchConfig {
        budget: u16::MAX,
        ..SearchConfig::default()
    };
    let max = match Searcher::new().search(&trie, &cm, b"a", &probe) {
        Err(SearchError::BudgetTooLarge { max, .. }) => max,
        other => panic!("expected BudgetTooLarge, got {other:?}"),
    };
    let at_max = SearchConfig {
        budget: max,
        ..SearchConfig::default()
    };
    assert!(Searcher::new().search(&trie, &cm, b"a", &at_max).is_ok());
    let over = SearchConfig {
        budget: max + 1,
        ..SearchConfig::default()
    };
    assert_eq!(
        Searcher::new().search(&trie, &cm, b"a", &over).unwrap_err(),
        SearchError::BudgetTooLarge {
            budget: max + 1,
            max
        }
    );
}

#[test]
fn a_searcher_can_be_reused_across_queries() {
    let trie = words();
    let cm = CostModel::qwerty();
    let cfg = SearchConfig::default();
    let mut s = Searcher::new();
    let first = s.search(&trie, &cm, b"hel", &cfg).unwrap().hits;
    let _ = s.search(&trie, &cm, b"abc", &cfg).unwrap();
    let again = s.search(&trie, &cm, b"hel", &cfg).unwrap().hits;
    assert_eq!(first, again);
}

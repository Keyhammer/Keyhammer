// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
//! Edge-case tests for the trie search.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use keyhammer::cost::{CostModel, whole_units};
use keyhammer::search::{MAX_QUERY_LEN, Ranking, SearchConfig, SearchError, Searcher};
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

/// Every combination of the subtree bound and the ranking mode.
fn modes() -> [(bool, Ranking); 4] {
    [
        (false, Ranking::Coarse),
        (true, Ranking::Coarse),
        (false, Ranking::Exact),
        (true, Ranking::Exact),
    ]
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
    for (tsb, ranking) in modes() {
        for q in cases {
            let cfg = SearchConfig {
                tsb,
                ranking,
                ..SearchConfig::default()
            };
            assert_eq!(
                run(&trie, q, &cfg),
                oracle_topk(&trie, &cm, q, cfg.budget, cfg.k, ranking),
                "q={q:?} tsb={tsb} ranking={ranking:?}"
            );
        }
    }
}

#[test]
fn empty_query_matches_the_oracle() {
    let trie = words();
    let cm = CostModel::qwerty();
    for (tsb, ranking) in modes() {
        let cfg = SearchConfig {
            tsb,
            ranking,
            ..SearchConfig::default()
        };
        assert_eq!(
            run(&trie, b"", &cfg),
            oracle_topk(&trie, &cm, b"", cfg.budget, cfg.k, ranking),
            "tsb={tsb} ranking={ranking:?}"
        );
    }
}

#[test]
fn length_mismatch_beyond_the_band_returns_nothing() {
    let trie = Trie::build(&[("cat", 1), ("dog", 1)]).unwrap();
    for (tsb, ranking) in modes() {
        let cfg = SearchConfig {
            tsb,
            ranking,
            ..SearchConfig::default()
        };
        assert!(
            run(&trie, b"aaaaaaaaaaaaaaaaaaaa", &cfg).is_empty(),
            "tsb={tsb} ranking={ranking:?}"
        );
    }
}

#[test]
fn k_zero_returns_nothing() {
    for (tsb, ranking) in modes() {
        let cfg = SearchConfig {
            k: 0,
            tsb,
            ranking,
            ..SearchConfig::default()
        };
        assert!(
            run(&words(), b"abc", &cfg).is_empty(),
            "tsb={tsb} ranking={ranking:?}"
        );
    }
}

#[test]
fn k_larger_than_the_match_count_returns_every_match() {
    let trie = words();
    let cm = CostModel::qwerty();
    for (tsb, ranking) in modes() {
        let cfg = SearchConfig {
            k: 1000,
            tsb,
            ranking,
            ..SearchConfig::default()
        };
        let got = run(&trie, b"hel", &cfg);
        assert_eq!(
            got,
            oracle_topk(&trie, &cm, b"hel", cfg.budget, 1000, ranking),
            "tsb={tsb} ranking={ranking:?}"
        );
        assert!(got.len() < 1000, "tsb={tsb} ranking={ranking:?}");
    }
}

#[test]
fn a_tiny_node_limit_truncates_and_returns_a_correct_prefix() {
    let trie = words();
    let cm = CostModel::qwerty();
    for (tsb, ranking) in modes() {
        let full = oracle_topk(&trie, &cm, b"hel", 32, 10, ranking);
        assert!(full.len() >= 2, "the fixture needs at least two matches");
        let mut saw_partial = false;
        let mut saw_complete = false;
        for max_nodes in 1..=trie.node_count() + 1 {
            let cfg = SearchConfig {
                max_nodes,
                tsb,
                ranking,
                ..SearchConfig::default()
            };
            let out = Searcher::new().search(&trie, &cm, b"hel", &cfg).unwrap();
            let got: Vec<(u32, u16)> = out.hits.iter().map(|h| (h.id, h.cost)).collect();
            assert!(
                got.len() <= full.len(),
                "max_nodes={max_nodes} tsb={tsb} ranking={ranking:?}"
            );
            assert_eq!(
                got,
                full[..got.len()].to_vec(),
                "max_nodes={max_nodes} tsb={tsb} ranking={ranking:?}"
            );
            if got.len() < full.len() {
                assert!(
                    out.stats.truncated,
                    "max_nodes={max_nodes} tsb={tsb} ranking={ranking:?}"
                );
            }
            saw_partial |= !got.is_empty() && got.len() < full.len();
            saw_complete |= got.len() == full.len() && !out.stats.truncated;
        }
        assert!(
            saw_partial,
            "tsb={tsb} ranking={ranking:?}: no node limit produced a non-empty strict prefix"
        );
        assert!(
            saw_complete,
            "tsb={tsb} ranking={ranking:?}: no node limit let the search complete"
        );
    }
}

#[test]
fn duplicate_terms_and_equal_weights_give_a_deterministic_order() {
    let trie = Trie::build(&[("bat", 5), ("cat", 5), ("hat", 5), ("cat", 9)]).unwrap();
    let cm = CostModel::qwerty();
    for (tsb, ranking) in modes() {
        let cfg = SearchConfig {
            tsb,
            ranking,
            ..SearchConfig::default()
        };
        let a = run(&trie, b"xat", &cfg);
        let b = run(&trie, b"xat", &cfg);
        assert_eq!(a, b, "tsb={tsb} ranking={ranking:?}");
        assert_eq!(
            a,
            oracle_topk(&trie, &cm, b"xat", cfg.budget, cfg.k, ranking),
            "tsb={tsb} ranking={ranking:?}"
        );
    }
}

#[test]
fn oversized_query_and_budget_are_typed_errors() {
    let trie = words();
    let cm = CostModel::qwerty();
    let long = vec![b'a'; MAX_QUERY_LEN + 1];
    for (tsb, ranking) in modes() {
        let ok = SearchConfig {
            tsb,
            ranking,
            ..SearchConfig::default()
        };
        let e = Searcher::new().search(&trie, &cm, &long, &ok).unwrap_err();
        assert_eq!(
            e,
            SearchError::QueryTooLong {
                len: MAX_QUERY_LEN + 1,
                max: MAX_QUERY_LEN
            },
            "tsb={tsb} ranking={ranking:?}"
        );
        let cfg = SearchConfig {
            budget: 80,
            tsb,
            ranking,
            ..SearchConfig::default()
        };
        let e = Searcher::new().search(&trie, &cm, b"a", &cfg).unwrap_err();
        assert!(
            matches!(e, SearchError::BudgetTooLarge { budget: 80, .. }),
            "tsb={tsb} ranking={ranking:?}"
        );
    }
}

#[test]
fn the_reported_budget_limit_is_the_largest_accepted_budget() {
    let trie = words();
    let cm = CostModel::qwerty();
    for (tsb, ranking) in modes() {
        let probe = SearchConfig {
            budget: u16::MAX,
            tsb,
            ranking,
            ..SearchConfig::default()
        };
        let max = match Searcher::new().search(&trie, &cm, b"a", &probe) {
            Err(SearchError::BudgetTooLarge { max, .. }) => max,
            other => {
                panic!("tsb={tsb} ranking={ranking:?}: expected BudgetTooLarge, got {other:?}")
            }
        };
        let at_max = SearchConfig {
            budget: max,
            tsb,
            ranking,
            ..SearchConfig::default()
        };
        assert!(
            Searcher::new().search(&trie, &cm, b"a", &at_max).is_ok(),
            "tsb={tsb} ranking={ranking:?}"
        );
        let over = SearchConfig {
            budget: max + 1,
            tsb,
            ranking,
            ..SearchConfig::default()
        };
        assert_eq!(
            Searcher::new().search(&trie, &cm, b"a", &over).unwrap_err(),
            SearchError::BudgetTooLarge {
                budget: max + 1,
                max
            },
            "tsb={tsb} ranking={ranking:?}"
        );
    }
}

#[test]
fn a_searcher_can_be_reused_across_queries() {
    let trie = words();
    let cm = CostModel::qwerty();
    for (tsb, ranking) in modes() {
        let cfg = SearchConfig {
            tsb,
            ranking,
            ..SearchConfig::default()
        };
        let mut s = Searcher::new();
        let first = s.search(&trie, &cm, b"hel", &cfg).unwrap().hits;
        let _ = s.search(&trie, &cm, b"abc", &cfg).unwrap();
        let again = s.search(&trie, &cm, b"hel", &cfg).unwrap().hits;
        assert_eq!(first, again, "tsb={tsb} ranking={ranking:?}");
    }
}

#[test]
fn equal_whole_units_are_ordered_by_weight() {
    // "hrllo": 'r' for 'e' is a neighbouring key (8), 'r' for 'a' is not (16).
    // Both costs round up to one unit of 16.
    let trie = Trie::build(&[("hello", 100), ("hallo", 200)]).unwrap();
    let cm = CostModel::qwerty();
    assert_eq!(cm.sub_cost(b'r', b'e', 1), 8);
    assert_eq!(cm.sub_cost(b'r', b'a', 1), 16);
    let hello = (0..trie.len() as u32)
        .find(|&id| trie.term(id) == "hello")
        .unwrap();
    let hallo = (0..trie.len() as u32)
        .find(|&id| trie.term(id) == "hallo")
        .unwrap();
    for tsb in [false, true] {
        let coarse = SearchConfig {
            tsb,
            ranking: Ranking::Coarse,
            ..SearchConfig::default()
        };
        // Same number of units: the heavier "hallo" first, costs stay exact.
        assert_eq!(
            run(&trie, b"hrllo", &coarse),
            vec![(hallo, 16), (hello, 8)],
            "tsb={tsb}"
        );
        let exact = SearchConfig {
            tsb,
            ranking: Ranking::Exact,
            ..SearchConfig::default()
        };
        assert_eq!(
            run(&trie, b"hrllo", &exact),
            vec![(hello, 8), (hallo, 16)],
            "tsb={tsb}"
        );
    }
}

#[test]
fn first_byte_edits_count_as_two_units() {
    // This pins the current documented behaviour; it does not claim that it is
    // ideal. Both terms are one edit away from "bat", but the edit on the
    // first byte of "cat" costs 16 x 1.5 = 24, which rounds up to two units,
    // so the lighter "bad" (16, one unit) ranks first under both rankings.
    let trie = Trie::build(&[("cat", 200), ("bad", 100)]).unwrap();
    let cm = CostModel::qwerty();
    assert_eq!(cm.sub_cost(b'b', b'c', 0), 24);
    assert_eq!(cm.sub_cost(b't', b'd', 2), 16);
    let id = |t: &str| (0..trie.len() as u32).find(|&i| trie.term(i) == t).unwrap();
    let (cat, bad) = (id("cat"), id("bad"));
    for (tsb, ranking) in modes() {
        let cfg = SearchConfig {
            tsb,
            ranking,
            ..SearchConfig::default()
        };
        assert_eq!(
            run(&trie, b"bat", &cfg),
            vec![(bad, 16), (cat, 24)],
            "tsb={tsb} ranking={ranking:?}"
        );
    }
}

#[test]
fn first_byte_units_depend_on_the_kind_of_edit() {
    // The x1.5 factor on the first byte: a neighbouring-key substitution
    // ('v' for 'b') costs 12, one unit; an ordinary substitution ('b' for
    // 'c') costs 24, two units.
    let cm = CostModel::qwerty();
    let neighbour = cm.sub_cost(b'v', b'b', 0);
    let ordinary = cm.sub_cost(b'b', b'c', 0);
    assert_eq!((neighbour, whole_units(neighbour)), (12, 1));
    assert_eq!((ordinary, whole_units(ordinary)), (24, 2));
    let transpose = cm.transpose_cost(0);
    assert_eq!((transpose, whole_units(transpose)), (18, 2));
    // The search reports the same true costs: "vat" is 12 from "bat".
    let trie = Trie::build(&[("bat", 1)]).unwrap();
    for (tsb, ranking) in modes() {
        let cfg = SearchConfig {
            tsb,
            ranking,
            ..SearchConfig::default()
        };
        assert_eq!(
            run(&trie, b"vat", &cfg),
            vec![(0, 12)],
            "tsb={tsb} ranking={ranking:?}"
        );
    }
}

#[test]
fn the_high_recall_preset_is_the_default_with_budget_48_and_the_bound_on() {
    let d = SearchConfig::default();
    let p = SearchConfig::high_recall();
    assert_eq!(d.budget, 32, "the default budget must stay 32");
    assert!(!d.tsb, "the default keeps the subtree bound off");
    assert_eq!(p.budget, 48);
    assert!(p.tsb);
    assert_eq!((p.k, p.max_nodes, p.ranking), (d.k, d.max_nodes, d.ranking));
}

#[test]
fn the_budget_limit_stays_64_and_the_preset_is_below_it() {
    let trie = words();
    let cm = CostModel::qwerty();
    let with = |budget| SearchConfig {
        budget,
        ..SearchConfig::default()
    };
    let mut s = Searcher::new();
    assert!(
        s.search(&trie, &cm, b"a", &SearchConfig::high_recall())
            .is_ok()
    );
    assert!(s.search(&trie, &cm, b"a", &with(64)).is_ok());
    assert_eq!(
        s.search(&trie, &cm, b"a", &with(65)).unwrap_err(),
        SearchError::BudgetTooLarge {
            budget: 65,
            max: 64
        }
    );
}

/// Six deletions at cost 8 each exactly reach the budget of 48 (band half-width
/// 6), so an under-wide band would miss the only term.
#[test]
fn budget_48_reaches_a_term_six_deletions_away() {
    let trie = Trie::build(&[("b", 1)]).unwrap();
    for tsb in [false, true] {
        let cfg = SearchConfig {
            tsb,
            ..SearchConfig::high_recall()
        };
        let got = run(&trie, b"bbbbbbb", &cfg);
        assert_eq!(got, vec![(0, 48)], "tsb={tsb}");
        assert_eq!(
            got,
            oracle_topk(
                &trie,
                &CostModel::qwerty(),
                b"bbbbbbb",
                48,
                cfg.k,
                cfg.ranking
            )
        );
    }
}

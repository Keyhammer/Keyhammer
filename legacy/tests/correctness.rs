// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

use keyhammer_legacy::FuzzyIndex;

#[test]
fn finds_exact_match() {
    let terms = vec!["apple", "banana", "cherry"];
    let idx = FuzzyIndex::build(&terms, 2).unwrap();
    let results = idx.search("apple", 5).unwrap();
    assert!(results.iter().any(|r| r.term == "apple" && r.hamming_distance == 0));
}

#[test]
fn finds_fuzzy_match() {
    let terms = vec!["javascript", "typescript", "coffeescript"];
    let idx = FuzzyIndex::build(&terms, 2).unwrap();
    // same length, 1 substitution: 'o' instead of 'a'
    let results = idx.search("jovascript", 5).unwrap();
    assert!(results.iter().any(|r| r.term == "javascript"));
}

#[test]
fn score_ordering_makes_sense() {
    let terms = vec!["the", "she", "them", "then"];
    let idx = FuzzyIndex::build(&terms, 2).unwrap();
    let results = idx.search("the", 10).unwrap();
    // exact match should be first
    assert_eq!(results[0].term, "the");
    assert_eq!(results[0].score, 1.0);
}

/// Verify that CGL tree finds the same results as brute-force for small inputs.
#[test]
fn matches_brute_force() {
    let terms = vec!["abc", "abd", "aec", "xbc", "xyz"];
    let idx = FuzzyIndex::build(&terms, 2).unwrap();

    let query = "abc";
    let results = idx.search(query, 100).unwrap();

    // brute-force: check all terms within hamming 2
    let q = query.as_bytes();
    let mut brute: Vec<&str> = terms.iter()
        .filter(|t| {
            let tb = t.as_bytes();
            let len = q.len().min(tb.len());
            let d: usize = (0..len).filter(|&i| q[i] != tb[i]).count();
            d <= 2
        })
        .copied()
        .collect();
    brute.sort();

    let mut tree_terms: Vec<String> = results.iter().map(|r| r.term.clone()).collect();
    tree_terms.sort();

    // every brute-force result should be in tree results
    for t in &brute {
        assert!(
            tree_terms.iter().any(|r| r == t),
            "brute-force found '{}' but tree did not. tree has: {:?}", t, tree_terms
        );
    }
}

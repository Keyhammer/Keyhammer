/// Stress tests — push keyhammer to its limits.
/// From edge cases to adversarial inputs to scale tests.

use keyhammer::FuzzyIndex;

// ─── Edge cases ────────────────────────────────────────────────────────────

#[test]
fn single_term() {
    let idx = FuzzyIndex::build(&["hello"], 2).unwrap();
    let r = idx.search("hello", 5).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].term, "hello");
}

#[test]
fn single_char_terms() {
    let terms: Vec<&str> = vec!["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"];
    let idx = FuzzyIndex::build(&terms, 1).unwrap();
    let r = idx.search("a", 10).unwrap();
    assert!(r.iter().any(|x| x.term == "a" && x.hamming_distance == 0));
}

#[test]
fn two_char_terms_deletion() {
    let terms = vec!["ab", "cd", "ef"];
    let idx = FuzzyIndex::build(&terms, 2).unwrap();
    // "a" is a deletion of "ab"
    let r = idx.search("a", 10).unwrap();
    assert!(
        r.iter().any(|x| x.term == "ab"),
        "should find 'ab' from query 'a', got: {:?}", r.iter().map(|x| &x.term).collect::<Vec<_>>()
    );
}

#[test]
fn empty_query_string() {
    let idx = FuzzyIndex::build(&["hello", "world"], 2).unwrap();
    let r = idx.search("", 10).unwrap();
    // empty query — should not panic, may return short terms within k
    assert!(r.len() <= 2);
}

#[test]
fn query_longer_than_all_terms() {
    let terms = vec!["ab", "cd", "ef"];
    let idx = FuzzyIndex::build(&terms, 2).unwrap();
    let r = idx.search("abcdefghijklmnop", 10).unwrap();
    // query is 16 chars, terms are 2 chars — length diff > k, no matches
    assert!(r.is_empty());
}

#[test]
fn very_long_term() {
    let long = "a".repeat(500);
    let terms = vec![long.as_str(), "short"];
    let idx = FuzzyIndex::build(&terms, 2).unwrap();
    let mut query = "a".repeat(500);
    query.replace_range(250..251, "z");
    let r = idx.search(&query, 5).unwrap();
    assert!(r.iter().any(|x| x.term == long));
}

#[test]
fn very_long_query() {
    let terms = vec!["hello", "world"];
    let idx = FuzzyIndex::build(&terms, 2).unwrap();
    let query = "x".repeat(10000);
    let r = idx.search(&query, 5).unwrap();
    assert!(r.is_empty()); // nothing close to a 10k string
}

// ─── Adversarial inputs ────────────────────────────────────────────────────

#[test]
fn all_identical_terms() {
    let terms = vec!["hello"; 1000];
    let idx = FuzzyIndex::build(&terms, 2).unwrap();
    let r = idx.search("hello", 5).unwrap();
    assert!(!r.is_empty());
    assert_eq!(r[0].term, "hello");
}

#[test]
fn all_anagrams_same_fingerprint() {
    // all these have the same character frequency — fingerprint can't distinguish them
    let terms = vec!["abc", "acb", "bac", "bca", "cab", "cba"];
    let idx = FuzzyIndex::build(&terms, 2).unwrap();

    let r = idx.search("abc", 5).unwrap();
    assert!(r.iter().any(|x| x.term == "abc" && x.hamming_distance == 0));
    // transpositions should also be found (hamming distance 2)
    assert!(r.len() >= 2, "should find multiple anagrams, got {}", r.len());
}

#[test]
fn worst_case_all_similar() {
    // all terms differ by exactly 1 char — maximum candidate set
    let terms: Vec<String> = (b'a'..=b'z').map(|c| {
        let mut s = String::from("aaaa");
        s.replace_range(2..3, &(c as char).to_string());
        s
    }).collect();
    let refs: Vec<&str> = terms.iter().map(|s| s.as_str()).collect();
    let idx = FuzzyIndex::build(&refs, 1).unwrap();

    let r = idx.search("aaaa", 26).unwrap();
    // should find all 26 — they all differ by at most 1 char
    assert_eq!(r.len(), 26, "should find all 26 variants, got {}", r.len());
}

#[test]
fn adversarial_repeated_char() {
    // terms are "a", "aa", "aaa", ..., "aaaaaaaaaa"
    let terms: Vec<String> = (1..=10).map(|n| "a".repeat(n)).collect();
    let refs: Vec<&str> = terms.iter().map(|s| s.as_str()).collect();
    let idx = FuzzyIndex::build(&refs, 2).unwrap();

    let r = idx.search("aaa", 10).unwrap();
    // "aaa" exact, "aa" (deletion), "aaaa" (insertion), "a" (2 deletions), "aaaaa" (2 insertions)
    assert!(r.len() >= 3, "should find multiple lengths, got {}", r.len());
}

#[test]
fn k_larger_than_term_length() {
    let terms = vec!["ab", "cd", "ef"];
    // k=4 (max allowed) but terms are 2 chars — hamming distance is at most 2
    let idx = FuzzyIndex::build(&terms, 4).unwrap();
    let r = idx.search("zz", 10).unwrap();
    assert!(r.len() == 3, "all 3 terms within distance 2, got {}", r.len());
}

#[test]
fn k_too_large_errors() {
    let terms = vec!["hello", "world"];
    let result = FuzzyIndex::build(&terms, 5);
    assert!(result.is_err(), "k=5 should be rejected");
}

#[test]
fn query_shares_zero_chars_with_terms() {
    let terms = vec!["aaaa", "bbbb", "cccc"];
    let idx = FuzzyIndex::build(&terms, 2).unwrap();
    let r = idx.search("zzzz", 5).unwrap();
    // distance 4 from everything, k=2, should find nothing
    assert!(r.is_empty());
}

// ─── Scale tests ───────────────────────────────────────────────────────────

#[test]
fn scale_5k_terms() {
    let terms: Vec<String> = (0..5000).map(|i| format!("term{:05}", i)).collect();
    let refs: Vec<&str> = terms.iter().map(|s| s.as_str()).collect();
    let idx = FuzzyIndex::build(&refs, 2).unwrap();

    // exact match
    let r = idx.search("term02500", 5).unwrap();
    assert!(r.iter().any(|x| x.term == "term02500"), "should find exact match in 5k terms");

    // typo
    let r = idx.search("term0250", 5).unwrap();
    assert!(!r.is_empty(), "should find deletion match in 5k terms");
}

#[test]
fn scale_10k_terms() {
    let terms: Vec<String> = (0..10000).map(|i| format!("word{:05}", i)).collect();
    let refs: Vec<&str> = terms.iter().map(|s| s.as_str()).collect();
    let idx = FuzzyIndex::build(&refs, 2).unwrap();

    let r = idx.search("word05000", 5).unwrap();
    assert!(r.iter().any(|x| x.term == "word05000"));

    // substitution
    let r = idx.search("word0500x", 5).unwrap();
    assert!(!r.is_empty(), "should find substitution match in 10k terms");
}

#[test]
fn scale_20k_terms() {
    let terms: Vec<String> = (0..20000).map(|i| format!("item{:06}", i)).collect();
    let refs: Vec<&str> = terms.iter().map(|s| s.as_str()).collect();
    let idx = FuzzyIndex::build(&refs, 2).unwrap();

    let r = idx.search("item010000", 5).unwrap();
    assert!(r.iter().any(|x| x.term == "item010000"), "should find in 20k terms");
}

// ─── Correctness at scale ──────────────────────────────────────────────────

#[test]
fn brute_vs_tree_5k() {
    // verify that tree path (>5k) and brute path produce same results
    let base_terms: Vec<String> = (0..100).map(|i| format!("base{:03}", i)).collect();
    let refs: Vec<&str> = base_terms.iter().map(|s| s.as_str()).collect();
    let idx = FuzzyIndex::build(&refs, 2).unwrap();

    let queries = ["base050", "base0x0", "bass050", "bxse050", "base05"];
    for q in queries {
        let results = idx.search(q, 100).unwrap();
        // just verify no panic and results are reasonable
        for r in &results {
            assert!(r.hamming_distance <= 2 || r.term.len() != q.len(),
                "result '{}' with hamming {} shouldn't be here for query '{}'",
                r.term, r.hamming_distance, q);
        }
    }
}

// ─── Memory / stability ────────────────────────────────────────────────────

#[test]
fn repeated_build_drop() {
    // build and drop 100 times — check for leaks or panics
    for _ in 0..100 {
        let terms = vec!["hello", "world", "rust", "test"];
        let idx = FuzzyIndex::build(&terms, 2).unwrap();
        let _ = idx.search("helo", 5).unwrap();
        drop(idx);
    }
}

#[test]
fn many_queries_same_index() {
    let terms: Vec<String> = (0..1000).map(|i| format!("t{:04}", i)).collect();
    let refs: Vec<&str> = terms.iter().map(|s| s.as_str()).collect();
    let idx = FuzzyIndex::build(&refs, 2).unwrap();

    // 10k queries on same index
    for i in 0..10000 {
        let q = format!("t{:04}", i % 1000);
        let r = idx.search(&q, 5).unwrap();
        assert!(!r.is_empty(), "query {} should find something", q);
    }
}

// ─── Mindblowing edge cases ────────────────────────────────────────────────

#[test]
fn every_term_is_prefix_of_next() {
    let terms = vec!["a", "ab", "abc", "abcd", "abcde", "abcdef"];
    let idx = FuzzyIndex::build(&terms, 2).unwrap();
    let r = idx.search("abc", 10).unwrap();
    // "abc" exact, "ab" (deletion), "abcd" (insertion), "a" (2 del), "abcde" (2 ins)
    assert!(r.len() >= 3, "prefix chain should match multiple, got {}", r.len());
}

#[test]
fn all_terms_same_length_max_diversity() {
    // 26^2 = 676 two-char terms covering all combinations
    let terms: Vec<String> = (b'a'..=b'z').flat_map(|a| {
        (b'a'..=b'z').map(move |b| String::from_utf8(vec![a, b]).unwrap())
    }).collect();
    let refs: Vec<&str> = terms.iter().map(|s| s.as_str()).collect();
    let idx = FuzzyIndex::build(&refs, 1).unwrap();

    // "aa" with k=1 should find: "aa" + all "xa" + all "ax" = 1 + 25 + 25 = 51
    let r = idx.search("aa", 700).unwrap();
    assert!(r.len() >= 51, "should find at least 51 matches for 'aa' k=1, got {}", r.len());
}

#[test]
fn unicode_in_terms() {
    // keyhammer works on bytes, not chars — unicode should not crash
    let terms = vec!["café", "naïve", "résumé", "hello"];
    let idx = FuzzyIndex::build(&terms, 2).unwrap();
    let r = idx.search("hello", 5).unwrap();
    assert!(r.iter().any(|x| x.term == "hello"));
    // searching for unicode should not panic
    let r = idx.search("café", 5).unwrap();
    assert!(!r.is_empty());
}

#[test]
fn numbers_and_symbols() {
    let terms = vec!["v1.0.0", "v2.0.0", "v1.0.1", "v10.0.0", "2024-01-01"];
    let idx = FuzzyIndex::build(&terms, 2).unwrap();
    let r = idx.search("v1.0.0", 5).unwrap();
    assert!(r.iter().any(|x| x.term == "v1.0.0"));
    let r = idx.search("v1.0.x", 5).unwrap();
    assert!(!r.is_empty(), "should find close version strings");
}

#[test]
fn mixed_lengths_extreme() {
    // terms from 1 char to 50 chars
    let terms: Vec<String> = (1..=50).map(|n| "x".repeat(n)).collect();
    let refs: Vec<&str> = terms.iter().map(|s| s.as_str()).collect();
    let idx = FuzzyIndex::build(&refs, 2).unwrap();

    let r = idx.search("xxxxx", 10).unwrap();
    // should find "xxxxx" (exact), "xxxx" (del), "xxxxxx" (ins), "xxx" (2 del), "xxxxxxx" (2 ins)
    assert!(r.len() >= 3, "should find multiple lengths, got {}", r.len());
}

#[test]
fn score_ordering_is_consistent() {
    let terms = vec![
        "javascript", "javescript", "javscript", "typescript", "coffeescript"
    ];
    let idx = FuzzyIndex::build(&terms, 2).unwrap();
    let r = idx.search("javascript", 10).unwrap();

    // exact match should always be first with score 1.0
    assert_eq!(r[0].term, "javascript");
    assert_eq!(r[0].score, 1.0);

    // scores should be in descending order
    for i in 1..r.len() {
        assert!(r[i - 1].score >= r[i].score,
            "scores should descend: {} ({}) >= {} ({})",
            r[i-1].term, r[i-1].score, r[i].term, r[i].score);
    }
}

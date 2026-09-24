// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
//! Tests for the flat trie.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use keyhammer::cost::class;
use keyhammer::trie::{BuildError, NO_TERM, Trie};

fn sample() -> Trie {
    Trie::build(&[("cat", 5), ("car", 9), ("cart", 1), ("dog", 3), ("cat", 7)]).unwrap()
}

#[test]
fn terms_are_sorted_and_deduplicated_keeping_the_highest_weight() {
    let t = sample();
    assert_eq!(t.len(), 4);
    let terms: Vec<&str> = (0..t.len() as u32).map(|i| t.term(i)).collect();
    assert_eq!(terms, ["car", "cart", "cat", "dog"]);
    assert_eq!(t.weight(2), 7); // "cat": max(5, 7)
}

#[test]
fn structure_has_shared_prefixes() {
    let t = sample();
    // root, c, d, ca, do, car, cat, dog, cart
    assert_eq!(t.node_count(), 9);
    let labels: Vec<u8> = t.children(0).map(|c| t.label(c)).collect();
    assert_eq!(labels, [b'c', b'd']);
}

#[test]
fn children_always_come_after_their_parent() {
    let t = sample();
    for v in 0..t.node_count() {
        for c in t.children(v) {
            assert!(c > v);
        }
    }
}

#[test]
fn aggregates_cover_the_whole_subtree() {
    let t = sample();
    assert_eq!(t.max_weight(0), 9);
    assert_eq!((t.len_min(0), t.len_max(0)), (3, 4));
    let mut all = 0u64;
    for b in b"cartdog" {
        all |= class(*b);
    }
    assert_eq!(t.below_mask(0), all);
    assert_eq!(t.term_id(0), NO_TERM);
}

#[test]
fn a_terminal_node_counts_its_own_length() {
    let t = sample();
    let car = t.children(t.children(t.children(0).start).start).start; // c -> a -> r
    assert_eq!(t.term_id(car), 0);
    assert_eq!((t.len_min(car), t.len_max(car)), (3, 4));
    assert_eq!(t.below_mask(car), class(b't')); // only "cart" continues below
}

#[test]
fn rejects_empty_input_and_empty_terms() {
    assert_eq!(Trie::build(&[]).unwrap_err(), BuildError::Empty);
    assert_eq!(
        Trie::build(&[("a", 1), ("", 2)]).unwrap_err(),
        BuildError::EmptyTerm
    );
}

#[test]
fn a_single_term_builds() {
    let t = Trie::build(&[("x", 0)]).unwrap();
    assert_eq!((t.len(), t.node_count()), (1, 2));
}

#[test]
fn non_ascii_bytes_are_kept_verbatim() {
    let t = Trie::build(&[("café", 1), ("cafe", 2)]).unwrap();
    assert_eq!(t.len(), 2);
    assert!((0..2u32).any(|i| t.term(i) == "café"));
}

#[test]
fn input_index_reports_the_kept_duplicate_with_the_highest_weight() {
    // sample(): ("cat", 5) at index 0 and ("cat", 7) at index 4.
    let t = sample();
    assert_eq!(t.term(2), "cat");
    assert_eq!(t.weight(2), 7);
    assert_eq!(t.input_index(2), 4);
}

#[test]
fn input_index_keeps_the_first_of_equal_weight_duplicates() {
    let t = Trie::build(&[("b", 1), ("a", 3), ("b", 3), ("a", 3), ("b", 3)]).unwrap();
    assert_eq!((t.term(0), t.term(1)), ("a", "b"));
    assert_eq!(t.input_index(0), 1);
    assert_eq!(t.input_index(1), 2);
}

#[test]
fn input_index_maps_sorted_positions_back_to_the_input() {
    let items = [("delta", 1), ("alpha", 2), ("charlie", 3), ("bravo", 4)];
    let t = Trie::build(&items).unwrap();
    let back: Vec<u32> = (0..t.len() as u32).map(|id| t.input_index(id)).collect();
    assert_eq!(back, [1, 3, 2, 0]);
    for id in 0..t.len() as u32 {
        let (term, weight) = items[t.input_index(id) as usize];
        assert_eq!((t.term(id), t.weight(id)), (term, weight));
    }
}

#[test]
fn input_index_of_an_out_of_range_id_is_u32_max() {
    let t = sample();
    assert_eq!(t.input_index(t.len() as u32), u32::MAX);
    assert_eq!(t.input_index(u32::MAX), u32::MAX);
}

#[test]
fn a_term_of_exactly_65535_bytes_builds() {
    let long = "a".repeat(usize::from(u16::MAX));
    let t = Trie::build(&[(long.as_str(), 1)]).unwrap();
    assert_eq!(t.len(), 1);
    assert_eq!(t.term(0).len(), usize::from(u16::MAX));
    assert_eq!(t.node_count(), usize::from(u16::MAX) + 1);
    assert_eq!((t.len_min(0), t.len_max(0)), (u16::MAX, u16::MAX));
}

#[test]
fn a_term_of_65536_bytes_is_too_long() {
    let long = "a".repeat(usize::from(u16::MAX) + 1);
    assert_eq!(
        Trie::build(&[(long.as_str(), 1)]).unwrap_err(),
        BuildError::TermTooLong
    );
    // Even when it is not the first term.
    assert_eq!(
        Trie::build(&[("ok", 1), (long.as_str(), 1)]).unwrap_err(),
        BuildError::TermTooLong
    );
}

#[test]
fn a_chain_of_prefix_terms_has_a_terminal_at_every_node() {
    let t = Trie::build(&[("abc", 1), ("a", 3), ("ab", 2)]).unwrap();
    // Ids follow the byte order: a, ab, abc.
    assert_eq!((t.term(0), t.term(1), t.term(2)), ("a", "ab", "abc"));
    assert_eq!(t.node_count(), 4); // root, a, ab, abc
    let a = t.children(0).start;
    let ab = t.children(a).start;
    let abc = t.children(ab).start;
    assert_eq!(t.children(0), 1..2);
    assert_eq!(t.children(abc).len(), 0);
    assert_eq!(
        [t.term_id(0), t.term_id(a), t.term_id(ab), t.term_id(abc)],
        [NO_TERM, 0, 1, 2]
    );
    assert_eq!((t.len_min(0), t.len_max(0)), (1, 3));
    assert_eq!((t.len_min(a), t.len_max(a)), (1, 3));
    assert_eq!((t.len_min(ab), t.len_max(ab)), (2, 3));
    assert_eq!((t.len_min(abc), t.len_max(abc)), (3, 3));
    // The weights differ, so the subtree maximum is the heaviest term below.
    assert_eq!(
        [t.max_weight(a), t.max_weight(ab), t.max_weight(abc)],
        [3, 2, 1]
    );
}

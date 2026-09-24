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

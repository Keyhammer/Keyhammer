// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
//! Tests for the fixed-point cost model.

use keyhammer::cost::{COST_UNIT, CostModel, class, whole_units};

#[test]
fn equal_bytes_cost_nothing() {
    let cm = CostModel::qwerty();
    assert_eq!(cm.sub_cost(b'a', b'a', 3), 0);
}

#[test]
fn adjacent_keys_are_cheaper_than_distant_keys() {
    let cm = CostModel::qwerty();
    assert_eq!(cm.sub_cost(b'r', b'e', 1), 8); // r and e are neighbours
    assert_eq!(cm.sub_cost(b'q', b'p', 1), 16); // far apart
    assert_eq!(cm.sub_cost(b'w', b'a', 1), 8); // diagonal neighbour on the row below
    assert_eq!(cm.sub_cost(b'q', b's', 1), 16); // q is not next to s
}

#[test]
fn adjacency_is_symmetric_for_every_letter_pair() {
    let cm = CostModel::qwerty();
    for a in b'a'..=b'z' {
        for b in b'a'..=b'z' {
            assert_eq!(
                cm.sub_cost(a, b, 2),
                cm.sub_cost(b, a, 2),
                "{} {}",
                a as char,
                b as char
            );
        }
    }
}

#[test]
fn edits_at_the_first_position_cost_one_and_a_half_times() {
    let cm = CostModel::qwerty();
    assert_eq!(cm.sub_cost(b'j', b'h', 0), 12); // 8 * 1.5
    assert_eq!(cm.sub_cost(b'q', b'p', 0), 24); // 16 * 1.5
    assert_eq!(cm.del_cost(b"abc", 0), 24);
    assert_eq!(cm.ins_cost(b'a', None, 0), 24);
    assert_eq!(cm.transpose_cost(0), 18);
}

#[test]
fn doubled_letters_are_cheaper_to_delete_or_insert() {
    let cm = CostModel::qwerty();
    assert_eq!(cm.del_cost(b"bookk", 4), 8); // 'k' repeated
    assert_eq!(cm.del_cost(b"bookk", 3), 16); // 'k' after 'o'
    assert_eq!(cm.ins_cost(b'n', Some(b'n'), 3), 8); // term has "nn"
    assert_eq!(cm.ins_cost(b'n', Some(b'i'), 3), 16);
}

#[test]
fn transposition_costs_twelve_away_from_the_start() {
    assert_eq!(CostModel::qwerty().transpose_cost(2), 12);
}

#[test]
fn minimum_costs_match_the_spec() {
    let cm = CostModel::qwerty();
    assert_eq!(cm.c_min(), 8);
    assert_eq!(cm.c_indel_min(), 8);
    assert_eq!(cm.c_transpose_min(), 12);
}

#[test]
fn non_letter_bytes_are_never_adjacent_and_never_panic() {
    let cm = CostModel::qwerty();
    assert_eq!(cm.sub_cost(b'1', b'2', 1), 16);
    assert_eq!(cm.sub_cost(0xC3, 0xA9, 1), 16);
    assert_eq!(cm.sub_cost(b' ', b'a', 1), 16);
}

#[test]
fn class_maps_each_lowercase_letter_to_its_own_bit() {
    let mut seen = 0u64;
    for b in b'a'..=b'z' {
        let c = class(b);
        assert_eq!(c.count_ones(), 1);
        assert_eq!(seen & c, 0, "collision on {}", b as char);
        seen |= c;
    }
}

#[test]
fn whole_units_rounds_up_to_whole_units_of_16() {
    assert_eq!(COST_UNIT, 16);
    for (cost, units) in [
        (0, 0),
        (1, 1),
        (8, 1),
        (12, 1),
        (16, 1),
        (17, 2),
        (24, 2),
        (32, 2),
        (33, 3),
    ] {
        assert_eq!(whole_units(cost), units, "cost={cost}");
    }
}

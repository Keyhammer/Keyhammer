// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
//! Tests for the fixed-point cost model.
#![allow(clippy::unwrap_used)]

use keyhammer::cost::{COST_UNIT, CostModel, Layout, class, whole_units};

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

#[test]
fn deleting_beyond_the_query_costs_a_plain_indel() {
    let cm = CostModel::qwerty();
    // No byte at `qpos`, so nothing can be doubled: the ordinary indel cost.
    assert_eq!(cm.del_cost(b"ab", 5), 16);
    assert_eq!(cm.del_cost(b"ab", 2), 16);
    assert_eq!(cm.del_cost(b"aa", 7), 16);
    // Position 0 of an empty query still gets the first-byte factor.
    assert_eq!(cm.del_cost(b"", 0), 24);
}

#[test]
fn a_doubled_letter_at_position_zero_is_cheap_but_carries_the_first_byte_factor() {
    let cm = CostModel::qwerty();
    // A cheap doubled-letter insertion (8) times 1.5 at the first position.
    assert_eq!(cm.ins_cost(b'a', Some(b'a'), 0), 12);
    // Without the repeat it is an ordinary indel (16) times 1.5.
    assert_eq!(cm.ins_cost(b'a', None, 0), 24);
    assert_eq!(cm.ins_cost(b'a', Some(b'b'), 0), 24);
    // Past the first position the factor is gone.
    assert_eq!(cm.ins_cost(b'a', Some(b'a'), 1), 8);
}

// ---- keyboard layouts ------------------------------------------------------

/// Hand-listed neighbours of each letter, `letter:neighbours`, for QWERTY.
const QWERTY_NEIGHBOURS: &str = "a:qwsz b:ghnv c:dfvx d:cerfsx e:drsw f:cdgrtv g:bfhtvy h:bgjnuy \
i:jkou j:hikmnu k:ijlmo l:kop m:jkn n:bhjm o:iklp p:lo q:aw r:deft s:adewxz t:fgry u:hijy \
v:bcfg w:aeqs x:cdsz y:ghtu z:asx";

/// QWERTZ is QWERTY with y and z exchanged.
const QWERTZ_NEIGHBOURS: &str = "a:qwsy b:ghnv c:dfvx d:cerfsx e:drsw f:cdgrtv g:bfhtvz h:bgjnuz \
i:jkou j:hikmnu k:ijlmo l:kop m:jkn n:bhjm o:iklp p:lo q:aw r:deft s:adewxy t:fgrz u:hijz \
v:bcfg w:aeqs x:cdsy y:asx z:ghtu";

const AZERTY_NEIGHBOURS: &str = "a:qz b:ghnv c:dfvx d:cefrsx e:drsz f:cdgrtv g:bfhtvy h:bgjnuy \
i:jkou j:hiknu k:ijlo l:kmop m:lp n:bhj o:iklp p:lmo q:aswz r:deft s:deqwxz t:fgry u:hijy \
v:bcfg w:qsx x:cdsw y:ghtu z:aeqs";

const DVORAK_NEIGHBOURS: &str = "a:o b:dhmx c:ghrt d:bfghix e:joqpu f:dgiy g:cdfh h:bcdgmt \
i:dfkuxy j:ekqu k:ijux l:nrs m:bhtw n:lrstvw o:aeq p:euy q:ejo r:clnt s:lnvz t:chmnrw \
u:eijkpy v:nswz w:mntv x:bdik y:fipu z:sv";

const COLEMAK_NEIGHBOURS: &str = "a:qrwz b:dhkv c:stvx d:bghjtv e:imnuy f:prsw g:djpt h:bdjkln \
i:eoy j:dghl k:bhmn l:hjnu m:ekn n:ehklmu o:i p:fgst q:aw r:afswxz s:cfprtx t:cdgpsv u:elny \
v:bcdt w:afqr x:crsz y:eiu z:arx";

fn hand_listed(table: &str) -> Vec<(u8, Vec<u8>)> {
    table
        .split_whitespace()
        .map(|entry| {
            let (k, v) = entry.split_once(':').unwrap();
            (k.as_bytes()[0], v.as_bytes().to_vec())
        })
        .collect()
}

fn neighbours_of(cm: &CostModel, a: u8) -> Vec<u8> {
    (b'a'..=b'z')
        .filter(|&b| b != a && cm.sub_cost(a, b, 1) == 8)
        .collect()
}

#[test]
fn geometry_reproduces_the_hand_listed_neighbours_of_every_layout() {
    // The tables above are written by hand from the key positions, not from
    // the generator. Dvorak and Colemak lose their punctuation neighbours.
    for (layout, table) in [
        (Layout::Qwerty, QWERTY_NEIGHBOURS),
        (Layout::Abnt2, QWERTY_NEIGHBOURS), // ç is outside a-z, so the same
        (Layout::Qwertz, QWERTZ_NEIGHBOURS),
        (Layout::Azerty, AZERTY_NEIGHBOURS),
        (Layout::Dvorak, DVORAK_NEIGHBOURS),
        (Layout::Colemak, COLEMAK_NEIGHBOURS),
    ] {
        let cm = CostModel::for_layout(layout);
        let listed = hand_listed(table);
        assert_eq!(listed.len(), 26, "{}", layout.name());
        for (letter, want) in listed {
            let mut want = want;
            want.sort_unstable();
            assert_eq!(
                neighbours_of(&cm, letter),
                want,
                "{} key {}",
                layout.name(),
                letter as char
            );
        }
    }
}

#[test]
fn every_layout_has_a_symmetric_relation() {
    for &layout in Layout::ALL {
        let cm = CostModel::for_layout(layout);
        for a in b'a'..=b'z' {
            assert_eq!(cm.sub_cost(a, a, 3), 0);
            for b in b'a'..=b'z' {
                assert_eq!(cm.sub_cost(a, b, 2), cm.sub_cost(b, a, 2));
                assert_eq!(
                    layout.are_neighbours(a as char, b as char),
                    layout.are_neighbours(b as char, a as char)
                );
            }
        }
    }
}

#[test]
fn abnt2_c_cedilla_is_a_neighbour_of_l_and_p_but_outside_a_to_z() {
    assert!(Layout::Abnt2.are_neighbours('ç', 'l'));
    assert!(Layout::Abnt2.are_neighbours('p', 'ç'));
    assert!(!Layout::Abnt2.are_neighbours('ç', 'k'));
    assert!(!Layout::Qwerty.are_neighbours('ç', 'l'));
    // Dropped from the cost model until non-a-z alphabets exist (#19).
    assert_eq!(
        CostModel::for_layout(Layout::Abnt2),
        CostModel::for_layout(Layout::Qwerty)
    );
}

#[test]
fn qwerty_is_byte_identical_to_the_pre_layout_table() {
    // The construction that CostModel::qwerty() used before layouts existed.
    fn link(adjacent: &mut [u32; 26], a: u8, b: u8) {
        let (ia, ib) = ((a - b'a') as usize, (b - b'a') as usize);
        adjacent[ia] |= 1 << ib;
        adjacent[ib] |= 1 << ia;
    }
    let rows: [&[u8]; 3] = [b"qwertyuiop", b"asdfghjkl", b"zxcvbnm"];
    let mut adjacent = [0u32; 26];
    for (r, row) in rows.iter().enumerate() {
        for (c, &key) in row.iter().enumerate() {
            if let Some(&next) = row.get(c + 1) {
                link(&mut adjacent, key, next);
            }
            if let Some(below) = rows.get(r + 1) {
                if let Some(&k) = below.get(c) {
                    link(&mut adjacent, key, k);
                }
                if let Some(&k) = c.checked_sub(1).and_then(|p| below.get(p)) {
                    link(&mut adjacent, key, k);
                }
            }
        }
    }
    let cm = CostModel::qwerty();
    for a in b'a'..=b'z' {
        for b in b'a'..=b'z' {
            let old = adjacent[(a - b'a') as usize] & (1 << (b - b'a')) != 0;
            assert_eq!(
                cm.sub_cost(a, b, 1),
                if a == b {
                    0
                } else if old {
                    8
                } else {
                    16
                }
            );
        }
    }
    assert_eq!(CostModel::for_layout(Layout::Qwerty), cm);
}

#[test]
fn minimum_costs_do_not_depend_on_the_layout() {
    for &layout in Layout::ALL {
        let cm = CostModel::for_layout(layout);
        assert_eq!(
            (cm.c_min(), cm.c_indel_min(), cm.c_transpose_min()),
            (8, 8, 12)
        );
    }
}

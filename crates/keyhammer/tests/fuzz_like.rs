// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
//! A deterministic, seeded stand-in for the cargo-fuzz targets: it feeds the
//! same properties (`tests/fuzz_props`) structured and raw bytes, so
//! `cargo test` exercises them on every platform, libFuzzer or not.
//!
//! Structured inputs put terms within a few edits of the query, so searches
//! return hits and ties; raw inputs check that nothing panics.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod fuzz_props;
mod support;

use support::Rng;

/// Iteration counts are cut under Miri, which is about 100x slower.
fn scale(n: usize) -> usize {
    if cfg!(miri) { n.div_ceil(200) } else { n }
}

fn letters(rng: &mut Rng, len: u64) -> Vec<u8> {
    (0..len).map(|_| b'a' + rng.below(6) as u8).collect()
}

/// A term within 0-2 edits of `q`, or a prefix/extension, or unrelated.
fn near(rng: &mut Rng, q: &[u8]) -> Vec<u8> {
    let mut t = q.to_vec();
    if rng.below(6) == 0 {
        let n = 1 + rng.below(8);
        return letters(rng, n);
    }
    for _ in 0..rng.below(3) {
        let n = t.len() as u64;
        match rng.below(7) {
            0 if n >= 2 => {
                let i = rng.below(n - 1) as usize;
                t.swap(i, i + 1);
            }
            1 => t.insert(rng.below(n + 1) as usize, b'a' + rng.below(6) as u8),
            2 if n >= 2 => {
                t.remove(rng.below(n) as usize);
            }
            3 if n >= 1 => t[0] = b'a' + rng.below(6) as u8,
            4 if n >= 1 => t.truncate(1 + rng.below(n) as usize),
            5 => {
                let n = 1 + rng.below(2);
                t.extend(letters(rng, n));
            }
            6 if n >= 1 => {
                let i = rng.below(n) as usize;
                t.insert(i, t[i]);
            }
            _ => {}
        }
    }
    if t.is_empty() { vec![b'a'] } else { t }
}

/// `[weight, term]` chunks separated by 0xFF, mostly near the query.
fn chunks(rng: &mut Rng, q: &[u8], max: u64) -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..2 + rng.below(max) {
        if i > 0 {
            out.push(0xFF);
        }
        out.push(rng.next() as u8);
        out.extend(near(rng, q));
    }
    out
}

/// Header `[flags, k, budget, qlen]` used by the oracle and tsb properties.
fn structured(rng: &mut Rng, max_terms: u64) -> Vec<u8> {
    let n = 1 + rng.below(8);
    let q = letters(rng, n);
    let mut d = vec![
        rng.next() as u8,
        rng.below(12) as u8,
        [7, 16, 24, 32, 48, 64][rng.below(6) as usize],
        q.len() as u8,
    ];
    d.extend(&q);
    d.extend(chunks(rng, &q, max_terms));
    d
}

/// Header `[k, budget(2), max_nodes(2), flags, qlen]` for `never_panics`.
fn structured_np(rng: &mut Rng) -> Vec<u8> {
    let n = rng.below(9);
    let q = letters(rng, n);
    let budget = if rng.below(8) == 0 {
        rng.below(200) as u8
    } else {
        rng.below(70) as u8
    };
    let max_nodes = if rng.below(2) == 0 {
        rng.below(60) as u8
    } else {
        200
    };
    let mut d = vec![
        rng.below(40) as u8,
        budget,
        0,
        max_nodes,
        0,
        rng.below(4) as u8,
        q.len() as u8,
    ];
    d.extend(&q);
    d.extend(chunks(rng, &q, 30));
    d
}

fn raw(rng: &mut Rng, max_len: u64) -> Vec<u8> {
    (0..rng.below(max_len + 1))
        .map(|_| match rng.below(8) {
            0 => 0xFF,
            1 | 2 => b'a' + rng.below(6) as u8,
            _ => rng.next() as u8,
        })
        .collect()
}

fn run(seed: u64, iters: usize, gen_structured: fn(&mut Rng) -> Vec<u8>, f: fn(&[u8])) {
    let mut rng = Rng::new(seed);
    for i in 0..scale(iters) {
        let d = if i % 5 == 4 {
            raw(&mut rng, 300)
        } else {
            gen_structured(&mut rng)
        };
        f(&d);
    }
}

#[test]
fn never_panics_on_arbitrary_bytes() {
    run(1, 20_000, structured_np, fuzz_props::never_panics);
}

#[test]
fn matches_the_oracle() {
    run(2, 4_000, |r| structured(r, 20), fuzz_props::oracle_equality);
}

#[test]
fn tsb_does_not_change_results() {
    run(
        3,
        8_000,
        |r| structured(r, 150),
        fuzz_props::tsb_equivalence,
    );
}

#[test]
fn prefix_matches_the_oracle() {
    run(
        4,
        4_000,
        |r| structured(r, 20),
        fuzz_props::prefix_oracle_equality,
    );
}

/// Characters that stress the normaliser: every folded block, the specials,
/// combining marks, joiners, bidirectional marks, emoji, NUL, the ends of the
/// scalar range.
const TRICKY: &[char] = &[
    'a',
    'Z',
    'ß',
    'İ',
    'ı',
    'ŉ',
    'ſ',
    'µ',
    'Æ',
    'œ',
    'Ç',
    'ã',
    'é',
    'Ÿ',
    'ÿ',
    '×',
    'ĸ',
    'ﬁ',
    'ﬃ',
    'ﬆ',
    '\u{300}',
    '\u{301}',
    '\u{307}',
    '\u{36F}',
    '\u{370}',
    '\u{200D}',
    '\u{200F}',
    '\u{202E}',
    '\u{FEFF}',
    '\u{0}',
    '😀',
    '👩',
    '\u{1F3FD}',
    'Ω',
    'Я',
    'ع',
    '\u{FFFD}',
    '\u{FFFF}',
    '\u{10FFFF}',
    ' ',
    '\u{2BC}',
    '\u{3BC}',
];

fn tricky(rng: &mut Rng) -> Vec<u8> {
    let mut d = vec![rng.below(20) as u8, rng.below(20) as u8];
    let mut s = String::new();
    for _ in 0..rng.below(16) {
        s.push(TRICKY[rng.below(TRICKY.len() as u64) as usize]);
    }
    d.extend(s.as_bytes());
    d
}

#[test]
fn normalize_is_idempotent_and_maps_stay_in_range() {
    run(5, 4_000, tricky, fuzz_props::normalize);
}

/// The structured oracle input with its letters shifted, so that over many
/// calls every character of the text alphabet occurs.
fn structured_text(rng: &mut Rng) -> Vec<u8> {
    let shift = rng.below(16) as u8;
    let mut d = structured(rng, 20);
    for b in d.iter_mut().skip(4) {
        if *b != 0xFF {
            *b = b.wrapping_add(shift);
        }
    }
    d
}

#[test]
fn text_matches_the_oracle() {
    run(6, 3_000, structured_text, fuzz_props::text_oracle_equality);
}

#[test]
fn highlight_matches_the_hit_cost() {
    run(7, 3_000, structured_text, fuzz_props::highlight);
}

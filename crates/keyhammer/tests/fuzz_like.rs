// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
//! A deterministic, seeded stand-in for the cargo-fuzz targets: it feeds the
//! same properties (`tests/fuzz_props`) pseudo-random and structured bytes, so
//! `cargo test` exercises them on every platform, libFuzzer or not.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod fuzz_props;
mod support;

use support::Rng;

/// Random bytes, biased towards lowercase letters and 0xFF term separators so
/// the trie gets real, overlapping terms.
fn bytes(rng: &mut Rng, max_len: u64) -> Vec<u8> {
    let len = rng.below(max_len + 1) as usize;
    let mode = rng.below(3);
    (0..len)
        .map(|_| match (mode, rng.below(8)) {
            (0, _) => rng.next() as u8,
            (_, 0) => 0xFF,
            (1, _) => b'a' + rng.below(6) as u8,
            _ => b'a' + rng.below(26) as u8,
        })
        .collect()
}

fn run(seed: u64, iters: usize, max_len: u64, f: fn(&[u8])) {
    let mut rng = Rng::new(seed);
    for _ in 0..iters {
        let d = bytes(&mut rng, max_len);
        f(&d);
    }
}

#[test]
fn never_panics_on_arbitrary_bytes() {
    run(1, 20_000, 300, fuzz_props::never_panics);
    // Header-shaped input: small budgets and node limits reach deeper code.
    let mut rng = Rng::new(11);
    for _ in 0..20_000 {
        let mut d = vec![
            rng.below(40) as u8,
            rng.below(80) as u8,
            0,
            rng.below(60) as u8,
            0,
            rng.below(2) as u8 * 2 + rng.below(2) as u8,
            rng.below(10) as u8,
            0,
        ];
        d.extend(bytes(&mut rng, 200));
        fuzz_props::never_panics(&d);
    }
}

#[test]
fn matches_the_oracle_on_random_bytes() {
    run(2, 3_000, 200, fuzz_props::oracle_equality);
}

#[test]
fn tsb_does_not_change_results() {
    run(3, 5_000, 400, fuzz_props::tsb_equivalence);
}

// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! With `--features calibration` the `calib` binary needs the experiment-only
//! patch applied to the core crate; stop early with a clear message otherwise.

fn main() {
    println!("cargo:rerun-if-changed=../crates/keyhammer/src/cost.rs");
    if std::env::var_os("CARGO_FEATURE_CALIBRATION").is_some() {
        let src = std::fs::read_to_string("../crates/keyhammer/src/cost.rs").unwrap_or_default();
        assert!(
            src.contains("qwerty_with"),
            "the calibration feature needs the experiment patch: run `git apply bench/experiments/cost-knobs.patch` from the repository root first (see docs/benchmarks/calibration.md)"
        );
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel
#![no_main]

#[path = "../../tests/fuzz_props/mod.rs"]
mod fuzz_props;
#[path = "../../tests/support/mod.rs"]
#[allow(dead_code)]
mod support;

libfuzzer_sys::fuzz_target!(|data: &[u8]| fuzz_props::normalize(data));

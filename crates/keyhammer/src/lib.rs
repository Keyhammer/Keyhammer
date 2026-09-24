// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! # keyhammer
//!
//! Typo-tolerant top-k search over a compact trie, with keyboard-aware edit
//! costs. This crate is `no_std` (it needs `alloc`), forbids `unsafe` and has
//! no runtime dependencies.
//!
//! Status: M0 prototype. Queries and terms are raw bytes, already lowercased.

#![no_std]

extern crate alloc;

pub mod cost;
pub mod search;
pub mod trie;

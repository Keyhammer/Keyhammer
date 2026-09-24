// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Robson Trasel

//! # keyhammer
//!
//! Typo-tolerant top-k search over a compact trie, with keyboard-aware edit
//! costs. This crate is `no_std` (it needs `alloc`), forbids `unsafe` and has
//! no runtime dependencies.
//!
//! Status: M0 prototype. Queries and terms are raw bytes, already lowercased.
//!
//! # Example
//!
//! ```
//! use keyhammer::cost::CostModel;
//! use keyhammer::search::{SearchConfig, Searcher};
//! use keyhammer::trie::Trie;
//!
//! let trie = Trie::build(&[
//!     ("javascript", 10),
//!     ("typescript", 10),
//!     ("python", 10),
//!     ("rust", 10),
//!     ("java", 10),
//! ])
//! .unwrap();
//! let costs = CostModel::qwerty();
//! let mut searcher = Searcher::new();
//!
//! // "javasript" skips the 'c' of "javascript".
//! let out = searcher
//!     .search(&trie, &costs, b"javasript", &SearchConfig::default())
//!     .unwrap();
//! let best = &out.hits[0];
//! assert_eq!(trie.term(best.id), "javascript");
//! assert_eq!(best.cost, 16);
//! ```

#![no_std]

extern crate alloc;

pub mod cost;
pub mod search;
pub mod trie;

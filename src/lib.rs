//! # keyhammer
//!
//! Fuzzy string search that knows how humans make typos.
//! 95-400x faster than FuseJS with typo-aware scoring.
//!
//! ## Quick start
//!
//! ```rust
//! use keyhammer::FuzzyIndex;
//!
//! let terms = vec!["javascript", "typescript", "python", "rust"];
//! let index = FuzzyIndex::build(&terms, 2).unwrap();
//!
//! let results = index.search("javasript", 5).unwrap();
//! assert_eq!(results[0].term, "javascript");
//! ```
//!
//! ## Architecture
//!
//! Two-phase search pipeline:
//! 1. **Candidate retrieval** — columnar Hamming scan + fingerprint matching
//! 2. **Ranking** — typo probability scorer with bit-level encoding
//!
//! For datasets above 5k terms, a lazy CGL tree provides sublinear query time.
//!
//! ## Author
//!
//! Robson Trasel ([@RobsonTrasel](https://github.com/RobsonTrasel))
//!
//! Based on: Bibbens, Borevitz, McCauley — [arXiv:2604.01307](https://arxiv.org/abs/2604.01307)

mod error;
mod altered;
mod tree;
mod inverter;
mod scorer;
mod variants;
mod columnar;
mod fingerprint;
mod encoding;
mod normalize;
mod query;
mod index;
mod document;

pub use error::{Error, Result};
pub use index::{FuzzyIndex, SearchResult, IndexStats};
pub use scorer::KeyboardLayout;
pub use encoding::TypoEncoding;
pub use document::{DocumentIndex, DocumentIndexBuilder, DocSearchResult, FieldMatch};

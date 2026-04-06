mod error;
mod altered;
mod tree;
mod inverter;
mod scorer;
mod variants;
mod columnar;
mod fingerprint;
mod index;

pub use error::{Error, Result};
pub use index::{FuzzyIndex, SearchResult, IndexStats};
pub use scorer::TypoScorer;

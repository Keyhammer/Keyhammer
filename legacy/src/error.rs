use std::fmt;

/// Errors returned by keyhammer operations.
#[derive(Debug)]
pub enum Error {
    /// Search radius exceeds the index's max k.
    RadiusExceedsMax { r: usize, k: usize },
    /// Cannot build an index from an empty term list.
    EmptyInput,
    /// The given k would cause exponential blowup in tree traversal.
    KTooLarge { k: usize, max: usize },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::RadiusExceedsMax { r, k } => {
                write!(f, "search radius r={r} exceeds index max k={k}")
            }
            Error::EmptyInput => write!(f, "input cannot be empty"),
            Error::KTooLarge { k, max } => {
                write!(f, "k={k} is too large (max {max})")
            }
        }
    }
}

impl std::error::Error for Error {}

/// Alias for `std::result::Result<T, keyhammer_legacy::Error>`.
pub type Result<T> = std::result::Result<T, Error>;

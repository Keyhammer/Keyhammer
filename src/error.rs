use std::fmt;

#[derive(Debug)]
pub enum Error {
    RadiusExceedsMax { r: usize, k: usize },
    EmptyInput,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::RadiusExceedsMax { r, k } => {
                write!(f, "search radius r={r} exceeds index max k={k}")
            }
            Error::EmptyInput => write!(f, "input cannot be empty"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

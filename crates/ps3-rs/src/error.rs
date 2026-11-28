//! Crate-wide error type.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("bad magic: expected {expected}, found {found}")]
    BadMagic { expected: String, found: String },

    #[error("unsupported format: {0}")]
    Unsupported(String),

    #[error("corrupt data: {0}")]
    Corrupt(String),

    #[error("wrong source: {0}")]
    WrongSource(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// The embedded disc model's error: a disc that is not one is corrupt.
    pub(crate) fn disc(error: std::io::Error) -> Error {
        match error.kind() {
            std::io::ErrorKind::InvalidData => Error::Corrupt(error.to_string()),
            _ => Error::Io(error),
        }
    }
}

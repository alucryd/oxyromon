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

    #[error("missing key: {0}")]
    MissingKey(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// A `Corrupt` error.
pub(crate) fn corrupt(message: impl Into<String>) -> Error {
    Error::Corrupt(message.into())
}

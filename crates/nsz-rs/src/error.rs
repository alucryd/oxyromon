//! Crate-wide error type.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("missing key: {0}")]
    MissingKey(String),

    #[error("invalid key: {0}")]
    InvalidKey(String),

    #[error("bad magic: expected {expected}, found {found}")]
    BadMagic { expected: String, found: String },

    #[error("verification failed: {0}")]
    Verification(String),

    #[error("unsupported format: {0}")]
    Unsupported(String),

    #[error("corrupt data: {0}")]
    Corrupt(String),

    #[error("hex decode error: {0}")]
    Hex(#[from] hex::FromHexError),
}

pub type Result<T> = std::result::Result<T, Error>;

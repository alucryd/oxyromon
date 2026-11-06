//! The error type of every crate embedding this one.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("bad magic: expected {expected}, found {found}")]
    BadMagic { expected: String, found: String },

    #[error("unsupported format: {0}")]
    Unsupported(String),

    #[error("invalid option: {0}")]
    InvalidOption(String),

    #[error("corrupt data: {0}")]
    Corrupt(String),

    /// A Wii disc whose partitions none of the keys given decrypts.
    #[error("missing key: {0}")]
    MissingKey(String),
}

pub type Result<T> = std::result::Result<T, Error>;

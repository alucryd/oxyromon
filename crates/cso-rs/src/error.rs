//! Crate-wide error type.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("bad magic: expected {expected}, found {found}")]
    BadMagic {
        expected: &'static str,
        found: String,
    },

    #[error("unsupported format: {0}")]
    Unsupported(String),

    #[error("invalid option: {0}")]
    InvalidOption(String),

    #[error("corrupt input: {0}")]
    Corrupt(String),

    #[error("compression failed: {0}")]
    Compression(String),

    #[error("could not write output: {0}")]
    Output(String),
}

pub type Result<T> = std::result::Result<T, Error>;

//! Crate-wide error type.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid option: {0}")]
    InvalidOption(String),

    #[error("corrupt data: {0}")]
    Corrupt(String),
}

pub type Result<T> = std::result::Result<T, Error>;

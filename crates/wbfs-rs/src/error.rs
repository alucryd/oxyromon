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

    /// A Wii disc whose partitions none of the keys given decrypts.
    #[error("missing key: {0}")]
    MissingKey(String),
}

impl From<rvz_rs::Error> for Error {
    fn from(error: rvz_rs::Error) -> Self {
        match error {
            rvz_rs::Error::Io(error) => Self::Io(error),
            rvz_rs::Error::BadMagic { expected, found } => Self::BadMagic { expected, found },
            rvz_rs::Error::Unsupported(what) | rvz_rs::Error::InvalidOption(what) => {
                Self::Unsupported(what)
            }
            rvz_rs::Error::Corrupt(what) => Self::Corrupt(what),
            rvz_rs::Error::MissingKey(what) => Self::MissingKey(what),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

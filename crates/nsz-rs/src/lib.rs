//! `nsz-rs`: NSP <-> NSZ zstd compression, part of oxyROMon.
//!
//! NSZs it writes are interchangeable with those of
//! [nsz](https://github.com/nicoboss/nsz) by Nico Bosshard (built on NUT by
//! Blake Warner), which it began as a port of.

pub mod compress;
pub mod crypto;
pub mod decompress;
pub mod error;
pub mod format;
pub mod keys;
pub mod pipeline;

pub use error::{Error, Result};

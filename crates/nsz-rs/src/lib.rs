//! `nsz-rs` — a Rust port of [nsz](https://github.com/nicoboss/nsz) by Nico
//! Bosshard (built on NUT by Blake Warner): NSP <-> NSZ zstd compression.

pub mod compress;
pub mod crypto;
pub mod decompress;
pub mod error;
pub mod format;
pub mod keys;
pub mod pipeline;

pub use error::{Error, Result};

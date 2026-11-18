//! Wii U specifics: a disc's layout and its encryption, its titles' TMDs and
//! tickets, the keys that open them, and a disc image read alike as a WUD or
//! a WUX.
//!
//! This crate is never published. wux-rs keeps a copy of this `src`
//! directory as `src/wiiu` and declares it a private module, so that the
//! crate it publishes stands alone; ore-rs, unpublished, declares it where it
//! is. So that it builds there as it builds here:
//!
//! - Paths within it go through `super::`, never `crate::`, as the crate
//!   root is the embedding crate's.
//! - Its [`Error`] is the embedding crate's error type, re-exported.
//! - What it depends on, each embedding crate depends on too.
//!
//! [`Error`]: error::Error

pub mod crypto;
pub mod disc;
pub mod error;
pub mod image;
pub mod io;
pub mod keys;
pub mod nus;
pub mod plan;
pub mod wux;

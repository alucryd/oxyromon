//! GameCube and Wii disc specifics: the disc's layout, Wii partitions and
//! their encryption, the file system, the common keys, and what a disc uses.
//!
//! This crate is never published. rvz-rs and wbfs-rs embed it instead: each
//! symlinks this `src` directory into its own as `src/gcwii` and declares it
//! a private module, so that the crate it publishes holds a copy, and stands
//! alone. So that it builds there as it builds here:
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
pub mod fst;
pub mod io;
pub mod keys;
pub mod scrub;
pub mod wii;

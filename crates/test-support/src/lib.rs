//! The synthetic discs and titles the format crates' tests are built on,
//! one copy for all of them: junk as Dolphin's documentation describes it,
//! hashes and encryption from scratch, and made-up keys, as no real disc
//! or key may ship.
//!
//! One feature per family, each a module of its own: the families name
//! helpers alike (`noise`, `COMMON_KEY`, `cbc_encrypt`) with shapes that
//! differ, so they must not meet. A crate enables the families its tests
//! use and builds nothing else.

#[cfg(feature = "gcwii")]
pub mod gcwii;
#[cfg(feature = "wiiu")]
pub mod wiiu;

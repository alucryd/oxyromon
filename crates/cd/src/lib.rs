// SPDX-License-Identifier: BSD-3-Clause
//! CD-ROM specifics shared by oxyROMon's format crates: raw sectors, what
//! of them can be generated, their EDC and ECC, Redump cue sheets, and the
//! ISO 9660 file system with its CD-ROM XA attributes.
//!
//! Never published: each crate using it embeds its source, so it holds no
//! error type of its own; what can fail returns `std::io::Result`, which
//! every host's `Error` converts from.

pub mod cue;
pub mod ecc;
pub mod edc;
pub mod iso9660;
pub mod sector;

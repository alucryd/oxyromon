//! CHD inspection, compression and decompression, part of oxyROMon.
//!
//! A CHD is MAME's Compressed Hunks of Data: one container for the logical
//! contents of a ROM, a CD, a DVD or a LaserDisc, split into hunks, each
//! compressed with one of several codecs, covered by hashes and described by
//! metadata. Files are interchangeable with those of `chdman`, which this
//! began as a port of.
//!
//! The library entry points arrive with the porting milestones: first
//! reading and inspecting, then writing DVDs and hard disks, then CDs. The
//! `chdrs` CLI, whose shell is the one of every oxyROMon tool, comes with
//! them; see the crate README.

mod bitstream;
mod cdrom;
mod codec;
mod container;
mod crc16;
mod ecc;
mod error;
mod flac;
mod huffman;
mod lzma;
mod writer;

pub use container::{Chd, ChdInfo, ChdType, MetadataEntry, VerifyOutcome, fourcc, sha1_hex};
pub use error::{Error, Result};
pub use writer::{create, create_hd};

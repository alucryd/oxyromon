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

mod avhuff;
mod avi;
mod bitstream;
// The CD-ROM specifics, shared with ore-rs: `src/cd` is a symlink to the cd
// crate's source, which publishing copies in
#[allow(dead_code, unused_imports)]
#[path = "cd/lib.rs"]
mod cd;
mod cdrom;
mod codec;
mod container;
mod crc16;
mod error;
mod extract;
mod flac;
mod huffman;
mod ld;
mod lzma;
mod vbi;
mod writer;

use cd::ecc;

pub use cdrom::{FRAME_SIZE as CD_FRAME_SIZE, input_size as cd_input_size};
pub use container::{Chd, ChdInfo, ChdType, MetadataEntry, VerifyOutcome, fourcc, sha1_hex};
pub use error::{Error, Result};
pub use extract::extract_cd;
pub use ld::{create_ld, extract_ld};
pub use writer::{create, create_cd, create_dvd, create_hd};

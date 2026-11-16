// SPDX-License-Identifier: MIT OR Apache-2.0
//! ORE, oxyROMon's ROM enclosure, for GameCube, Wii, PlayStation 2 and PSP
//! discs and CDs: part of oxyROMon.
//!
//! An ORE holds a disc as Redump hashes it, and gives it back byte for byte.
//! Wii partitions are stored decrypted and without their hashes, padding as
//! the seed that generates it, and runs of zeros as their length, as RVZ
//! does; what is left is cut into content-defined chunks of 16 KiB on
//! average, each stored once however often the disc repeats it, and the
//! chunks are compressed in groups of 1 or 4 MiB with Zstandard or LZMA2.
//! The disc's CRC-32, MD5, SHA-1 and BLAKE3 are recorded, so it can be
//! checked against a DAT without being decoded.
//!
//! A CD, given as its cue sheet and a BIN a track, is stored the same way
//! once each data sector's sync, address, EDC and ECC are taken out to be
//! generated again, its audio tracks in FLAC; it comes back as those files,
//! with its SBI if it had one.
//!
//! ```no_run
//! use std::path::Path;
//! use ore_rs::Options;
//!
//! let (iso, ore) = (Path::new("game.iso"), Path::new("game.ore"));
//!
//! // A Wii disc needs the Wii common key, which ships with nothing here
//! let keys = ore_rs::load_keys(Path::new("keys.bin")).unwrap();
//! ore_rs::compress(iso, ore, &Options::default(), &keys, &mut |_| {}).unwrap();
//! ore_rs::decompress(ore, Path::new("out.iso"), &mut |_| {}).unwrap();
//! ```
//!
//! The format's specification and its profiles are unpublished while the
//! format is a draft.

// shortcut: without the writer, what only encoding uses (the codecs'
// encoders, the CD map's writer, the store index's saving) is compiled and
// unused rather than gated item by item; gate it if a host needs it smaller
#![cfg_attr(not(feature = "write"), allow(dead_code))]

// The CD-ROM specifics, shared with chd-rs: `src/cd` is a symlink to the cd
// crate's source, which publishing copies in
#[allow(dead_code, unused_imports)]
#[path = "cd/lib.rs"]
mod cd;
#[cfg(feature = "write")]
mod cdc;
mod cdtrack;
mod codec;
mod format;
// The GameCube and Wii disc specifics, shared with rvz-rs and wbfs-rs:
// `src/gcwii` is a symlink to the gcwii crate's source, which publishing
// copies in
#[allow(dead_code, unused_imports)]
#[cfg(feature = "write")]
#[path = "gcwii/lib.rs"]
mod gcwii;
// Without the writer, only what reading needs of it, which excludes what is
// ported from Dolphin, GPL-2.0-or-later: its disc model and file system
#[cfg(not(feature = "write"))]
#[allow(dead_code, unused_imports)]
mod gcwii {
    pub mod crypto;
    pub mod error;
    pub mod io;
    pub mod lfg;
    pub mod wii;
}
mod pack;
mod parity;
#[cfg(feature = "write")]
mod ps2audio;
mod range;
mod reader;
#[cfg(feature = "write")]
mod segment;
mod store;
mod vag;
mod video;
// The Wii U disc specifics, shared with wux-rs: `src/wiiu` is a symlink to
// the wiiu crate's source, GPL-3.0-or-later, which only writing needs
#[allow(dead_code, unused_imports)]
#[cfg(feature = "write")]
#[path = "wiiu/lib.rs"]
mod wiiu;
mod wiiu_crypt;
#[cfg(feature = "write")]
mod writer;
mod xa;

use gcwii::{crypto, error, io, lfg, wii};
#[cfg(feature = "write")]
use gcwii::{disc, fst, keys};

pub use crypto::Key;
pub use error::{Error, Result};
pub use format::{Codec, Profile};
#[cfg(feature = "write")]
pub use keys::{is_bootmii, load_keys, parse_key_file, parse_keys, parse_otp};
pub use reader::{Reader, Sizes, Stats, Track, TrackMode, decompress, is_ore};
#[cfg(feature = "write")]
pub use store::{Compacted, compact, create_store, set_parity};
pub use store::{STORE_DIR, StoreChecked, StoreOptions, check_store, find_store};
#[cfg(feature = "write")]
pub use writer::{
    CARTRIDGE_PLATFORMS, Options, StoreBatch, attach, cartridge_header_matches, compress,
    compress_to_store, default_codec, detach, is_title_folder, replace_cue,
};

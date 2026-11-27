// SPDX-License-Identifier: MIT OR Apache-2.0
//! ORE, oxyROMon's ROM enclosure, for GameCube, Wii, PlayStation 2, PlayStation
//! 3 and PSP discs and CDs: part of oxyROMon.
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
//! The format is ORE 1.0, specified in the crate's `SPEC.md`, its
//! conformance vectors in `spec/vectors`.

// shortcut: without the writer, what only encoding uses (the codecs'
// encoders, the CD map's writer, the store index's saving) is compiled and
// unused rather than gated item by item; gate it if a host needs it smaller
#![cfg_attr(not(feature = "write"), allow(dead_code))]

// The CD-ROM specifics, shared with chd-rs: the cd crate's source, reached
// outside this crate, which is never published
#[allow(dead_code, unused_imports)]
#[path = "../../cd/src/lib.rs"]
mod cd;
#[cfg(feature = "write")]
mod cdc;
mod cdtrack;
mod codec;
mod format;
// The GameCube and Wii disc specifics, shared with rvz-rs and wbfs-rs:
// the gcwii crate's source, reached outside this crate
#[allow(dead_code, unused_imports)]
#[cfg(feature = "write")]
#[path = "../../gcwii/src/lib.rs"]
mod gcwii;
// Without the writer, only what reading needs of it, which excludes what is
// ported from Dolphin, GPL-2.0-or-later: its disc model and file system
#[cfg(not(feature = "write"))]
#[allow(dead_code, unused_imports)]
#[path = "../../gcwii/src"]
mod gcwii {
    pub mod crypto;
    pub mod error;
    pub mod io;
    pub mod lfg;
    pub mod wii;
}
mod pack;
mod parity;
mod precache;
#[cfg(feature = "write")]
mod ps2audio;
// The PS3 disc specifics, shared with ps3-rs: the ps3 crate's source,
// MIT OR Apache-2.0, reached outside this crate, which the reader puts a
// disc's encryption back with
#[allow(dead_code, unused_imports)]
#[path = "../../ps3/src/lib.rs"]
mod ps3;
mod range;
mod reader;
#[cfg(feature = "write")]
mod segment;
mod store;
mod vag;
mod video;
// The Wii U disc specifics, shared with wux-rs: the wiiu crate's source,
// GPL-3.0-or-later, which only writing needs, reached outside this crate
#[allow(dead_code, unused_imports)]
#[cfg(feature = "write")]
#[path = "../../wiiu/src/lib.rs"]
mod wiiu;
mod wiiu_crypt;
#[cfg(feature = "write")]
mod writer;
mod xa;
mod xbox;

use gcwii::{crypto, error, io, lfg, wii};
#[cfg(feature = "write")]
use gcwii::{disc, fst, keys};

pub use crypto::Key;
pub use error::{Error, Result};
pub use format::{Codec, Profile};
#[cfg(feature = "write")]
pub use keys::{is_bootmii, load_keys, parse_key_file, parse_keys, parse_otp};
pub use precache::Progress;
pub use reader::{
    DEFAULT_READ_AHEAD, OreWeight, READ_AHEAD_OFF, Reader, Savings, Sizes, Stats, Track, TrackMode,
    decompress, game_sizes, is_ore, ore_weights, savings, shares,
};
pub use store::Index;
#[cfg(feature = "write")]
pub use store::{Compacted, Recounted, compact, create_store, recount, set_parity};
pub use store::{STORE_DIR, StoreChecked, StoreOptions, check_store, find_store, store_size};
pub use store::{StoreStats, store_stats};
#[cfg(feature = "write")]
pub use writer::{
    CARTRIDGE_PLATFORMS, FLOPPY_PLATFORMS, Options, StoreBatch, attach, cartridge_header_matches,
    compress, compress_to_store, default_codec, detach, is_title_folder, replace_cue,
};

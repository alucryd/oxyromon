//! ORE, oxyROMon's ROM enclosure, for GameCube and Wii discs: part of
//! oxyROMon.
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
//! The format is specified in oxyROMon's `formats/design/01-format-spec.md`,
//! and its GameCube and Wii profile in `07-gamecube-wii-profile.md`.

mod cdc;
mod codec;
mod format;
// The GameCube and Wii disc specifics, shared with rvz-rs and wbfs-rs:
// `src/gcwii` is a symlink to the gcwii crate's source, which publishing
// copies in
#[allow(dead_code, unused_imports)]
#[path = "gcwii/lib.rs"]
mod gcwii;
mod pack;
mod reader;
mod segment;
mod store;
mod writer;

use gcwii::{crypto, disc, error, fst, io, keys, lfg, wii};

pub use crypto::Key;
pub use error::{Error, Result};
pub use format::{Codec, Profile};
pub use keys::{is_bootmii, load_keys, parse_key_file, parse_keys, parse_otp};
pub use reader::{Reader, Sizes, decompress};
pub use store::{Purged, STORE_DIR, StoreOptions, create_store, find_store, purge};
pub use writer::{Options, Stats, attach, compress, compress_to_store, detach, is_ore};

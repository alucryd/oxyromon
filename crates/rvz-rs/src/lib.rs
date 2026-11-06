//! RVZ compression and decompression for GameCube and Wii discs, scrubbing
//! included, part of oxyROMon.
//!
//! RVZ is the disc image format of [Dolphin], which this is a port of: chunks
//! of the disc compressed with Zstandard, bzip2, LZMA or LZMA2, Wii partition
//! data stored decrypted and without its hashes, and the padding discs are
//! filled with stored as the seed that generates it. Files are
//! interchangeable with Dolphin's, and with Zstandard or bzip2, or none, the
//! same byte for byte.
//!
//! ```no_run
//! use std::path::Path;
//! use rvz_rs::Options;
//!
//! let iso = Path::new("game.iso");
//! let rvz = Path::new("game.rvz");
//!
//! // A Wii disc needs the Wii common key, which ships with nothing here
//! let keys = rvz_rs::load_keys(Path::new("keys.bin")).unwrap();
//! rvz_rs::compress(iso, rvz, &Options::default(), &keys, &mut |_| {}).unwrap();
//! rvz_rs::decompress(rvz, Path::new("out.iso"), &mut |_| {}).unwrap();
//! ```
//!
//! Reading an RVZ needs no key, nor does rewriting one: it keeps each Wii
//! partition's title key. [`Reader`] reads one as the disc it holds, for
//! hashing without writing the disc out.
//!
//! [Dolphin]: https://dolphin-emu.org

mod compression;
mod crypto;
mod disc;
mod error;
mod format;
mod fst;
mod io;
mod keys;
mod lfg;
mod reader;
mod scrub;
mod wii;
mod writer;

use std::fs::File;
use std::io::Write;
use std::path::Path;

use disc::{Iso, Source, Volume};
use scrub::Scrubber;

pub use compression::Compression;
pub use crypto::Key;
pub use error::{Error, Result};
pub use keys::{is_bootmii, load_keys, parse_key_file, parse_keys, parse_otp};
pub use reader::Reader;
pub use scrub::CLUSTER_SIZE;
pub use writer::{Options, Stats, compress, is_rvz, is_valid_chunk_size};

/// How much is decompressed at a time: a Wii group, which is what is
/// hashed and encrypted together.
const STEP: u64 = wii::GROUP_TOTAL_SIZE;

/// Which 32 KiB clusters of `input`, a raw GameCube or Wii disc or an RVZ,
/// anything on the disc points to: what scrubbing keeps, the clusters wit
/// keeps too. A disc without a boot DOL, which scrubbing refuses, is used
/// throughout: its own apploader loads code from where no file points.
/// `keys` are the Wii common keys, as for [`compress`].
pub fn used_clusters(input: &Path, keys: &[Key]) -> Result<Vec<bool>> {
    let mut source: Box<dyn Source> = if is_rvz(input)? {
        Box::new(Reader::open(input)?)
    } else {
        Box::new(Iso::open(input)?)
    };
    let mut volume = Volume::new(&mut *source, keys)?
        .ok_or_else(|| Error::Unsupported("not a GameCube or Wii disc".into()))?;
    Ok(Scrubber::new(&mut volume, &mut *source, true)?.used())
}

/// Write an RVZ out as the disc it holds.
///
/// `progress` is called with the input bytes consumed since the last call.
pub fn decompress(input: &Path, output: &Path, progress: &mut dyn FnMut(u64)) -> Result<Stats> {
    let input_size = std::fs::metadata(input)?.len();
    let mut reader = Reader::open(input)?;
    let iso_size = reader.size();
    let part = io::part(output);
    let result = (|| {
        let mut file = File::create(&part)?;
        let mut buf = vec![0; STEP as usize];
        let mut offset = 0;
        let mut reported = 0;
        while offset < iso_size {
            let length = STEP.min(iso_size - offset) as usize;
            reader.read_at(offset, &mut buf[..length])?;
            file.write_all(&buf[..length])?;
            offset += length as u64;
            // Scale the progress through the disc to the file
            let consumed = (offset as u128 * input_size as u128 / iso_size as u128) as u64;
            progress(consumed - reported);
            reported = consumed;
        }
        file.sync_all()?;
        Ok(Stats {
            input_size,
            output_size: iso_size,
        })
    })();
    io::finish(&part, output, result)
}

//! CSO and ZSO compression and decompression.
//!
//! A port of [maxcso]'s core, minus the GUI and the formats nobody uses. CSO v1
//! stores DEFLATE blocks, ZSO stores LZ4 blocks; both share one container of a
//! 24-byte header, an index of little-endian offsets, and the block data.
//!
//! ```no_run
//! use std::path::Path;
//! use cso_rs::{CompressOptions, DecompressOptions, Format};
//!
//! let iso = Path::new("game.iso");
//! let cso = Path::new("game.cso");
//! let zso = Path::new("game.zso");
//!
//! cso_rs::compress(iso, cso, &CompressOptions::new(Format::Cso), &mut |_| {}).unwrap();
//! cso_rs::compress(iso, zso, &CompressOptions::new(Format::Zso), &mut |_| {}).unwrap();
//! cso_rs::decompress(cso, Path::new("out.iso"), &DecompressOptions::default(), &mut |_| {}).unwrap();
//! ```
//!
//! Compression takes a raw ISO. To turn a CSO into a ZSO, decompress first and
//! then compress; there is no direct transcode.
//!
//! [maxcso]: https://github.com/mattlewis92/maxcso

mod compress;
mod decompress;
mod error;
mod format;
mod io;
mod lz4;
mod method;

pub use compress::{
    compress, CompressOptions, DEFAULT_BLOCK_SIZE, LARGE_BLOCK_SIZE,
};
pub use decompress::{decompress, DecompressOptions};
pub use error::{Error, Result};
pub use format::{
    Format, Header, Codec, HEADER_SIZE, INDEX_OFFSET_MASK, INDEX_UNCOMPRESSED, MAX_BLOCK_SIZE,
    SECTOR_SIZE,
};
pub use method::{BlockKind, CostModel, Methods};

/// Byte-level progress, reported from the calling thread after each block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Bytes of the source consumed so far.
    pub done: u64,
    /// Total bytes the source will contribute.
    pub total: u64,
    /// Bytes placed in the output so far.
    pub written: u64,
}

impl Progress {
    pub fn fraction(&self) -> f64 {
        if self.total == 0 {
            1.0
        } else {
            self.done as f64 / self.total as f64
        }
    }
}

/// Sizes of the two files a run touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub input_size: u64,
    pub output_size: u64,
}

impl Stats {
    /// Compressed size as a percentage of the original.
    pub fn ratio_percent(&self) -> f64 {
        if self.input_size == 0 {
            0.0
        } else {
            self.output_size as f64 * 100.0 / self.input_size as f64
        }
    }
}

/// Read the header of a CSO or ZSO without decompressing it.
pub fn probe(path: &std::path::Path) -> Result<Header> {
    let file = std::fs::File::open(path)?;
    let size = file.metadata()?.len();
    decompress::read_header(&file, size)
}

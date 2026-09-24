//! ISO -> CSO/ZSO.
//!
//! Blocks are independent, so they are compressed in parallel waves and then
//! written back in order from a single writer. The index and header go on at the
//! end, once every block's final offset is known.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

use rayon::iter::{IntoParallelIterator, ParallelIterator};
use rayon::{ThreadPool, ThreadPoolBuilder};

use crate::Stats;
use crate::error::{Error, Result};
use crate::format::{
    Format, HEADER_SIZE, INDEX_UNCOMPRESSED, SECTOR_SIZE, align_up, block_count,
    compute_index_shift,
};
use crate::io::{finish, part, read_exact_at};
use crate::method::{self, Block};

/// CSO block size: an ARK-5 PSP plays 8 KiB blocks, and PPSSPP and PCSX2 read
/// them, for a CSO 3.8% smaller than with 2 KiB (on Patapon). Larger blocks
/// shrink it little more, and make every read decompress more.
const CSO_BLOCK_SIZE: u32 = 8192;
/// CSO block size from 2 GiB. Only PS2 DVDs get that big, never a UMD, and
/// PCSX2 reads any power-of-two block size.
const CSO_LARGE_BLOCK_SIZE: u32 = 16384;
const LARGE_THRESHOLD: u64 = 0x8000_0000;
/// ZSO block size: Open PS2 Loader hard-codes 2 KiB blocks, and silently
/// misreads anything else.
const ZSO_BLOCK_SIZE: u32 = 2048;

/// The block size `format` defaults to, for an input of `size` bytes.
pub fn default_block_size(format: Format, size: u64) -> u32 {
    match format {
        Format::Cso if size >= LARGE_THRESHOLD => CSO_LARGE_BLOCK_SIZE,
        Format::Cso => CSO_BLOCK_SIZE,
        Format::Zso => ZSO_BLOCK_SIZE,
    }
}

/// Everything that shapes a CSO/ZSO write. The format decides the encoder:
/// zlib for CSO, LZ4 HC for ZSO.
pub struct CompressOptions {
    pub format: Format,
    /// `None` picks [`default_block_size`].
    pub block_size: Option<u32>,
    /// 0 means let rayon decide.
    pub threads: usize,
}

impl CompressOptions {
    pub fn new(format: Format) -> CompressOptions {
        CompressOptions {
            format,
            block_size: None,
            threads: 0,
        }
    }
}

/// Compress a raw ISO into a CSO or ZSO file.
///
/// `progress` is called with each newly compressed chunk of the input, in
/// bytes; the calls add up to the input file size. An existing `output` is
/// only replaced once the new one is complete, and a failed run leaves nothing
/// behind.
pub fn compress(
    input: &Path,
    output: &Path,
    options: &CompressOptions,
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    let in_file = File::open(input)?;
    let src_size = in_file.metadata()?.len();
    if src_size == 0 {
        return Err(Error::InvalidOption("input file is empty".into()));
    }
    if src_size % u64::from(SECTOR_SIZE) != 0 {
        return Err(Error::InvalidOption(format!(
            "input size {src_size} is not a multiple of the {SECTOR_SIZE} byte sector size"
        )));
    }

    let block_size = resolve_block_size(options.block_size, options.format, src_size)?;
    let blocks = block_count(src_size, block_size);
    let index_len = (blocks + 1) * 4;
    let unaligned = HEADER_SIZE as u64 + index_len;
    let shift = compute_index_shift(unaligned + src_size);
    let align = 1u64 << shift;
    let data_start = align_up(unaligned, align);

    let pool = build_pool(options.threads)?;
    let wave = pool.current_num_threads().max(1) * 4;

    let part = part(output);
    let out_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&part)?;
    // Everything from here on writes to the `.part` file, only moved into place
    // once complete.
    let result = (|| {
        let mut writer = BufWriter::with_capacity(256 * 1024, out_file);
        writer.seek(SeekFrom::Start(data_start))?;

        let mut index = vec![0u32; (blocks + 1) as usize];
        let mut dst = data_start;

        for wave_start in (0..blocks).step_by(wave) {
            let wave_end = (wave_start + wave as u64).min(blocks);
            let blocks_out: Vec<Block> = pool.install(|| {
                (wave_start..wave_end)
                    .into_par_iter()
                    .map(|i| {
                        let off = i * u64::from(block_size);
                        let want = u64::from(block_size).min(src_size - off) as usize;
                        // The tail is zero-padded to a whole block, as maxcso does.
                        let mut buf = vec![0u8; block_size as usize];
                        read_exact_at(&in_file, &mut buf[..want], off)?;
                        Ok(method::compress_block(&buf, options.format, align))
                    })
                    .collect::<Result<Vec<_>>>()
            })?;

            for (offset_in_wave, block) in blocks_out.into_iter().enumerate() {
                let i = (wave_start + offset_in_wave as u64) as usize;
                let flag = if block.raw { INDEX_UNCOMPRESSED } else { 0 };
                index[i] = ((dst >> shift) as u32) | flag;

                writer.write_all(&block.data)?;
                dst += block.data.len() as u64;

                // Pad to the index alignment. Seek rather than write, so a large
                // shift leaves a sparse hole instead of gigabytes of zeroes.
                let padded = align_up(dst, align);
                if padded > dst {
                    writer.seek(SeekFrom::Current((padded - dst) as i64))?;
                    dst = padded;
                }

                let start = i as u64 * u64::from(block_size);
                progress(u64::from(block_size).min(src_size - start));
            }
        }

        index[blocks as usize] = (dst >> shift) as u32;

        let mut out_file = writer.into_inner().map_err(|e| Error::Io(e.into_error()))?;
        out_file.seek(SeekFrom::Start(0))?;
        let header = crate::format::Header {
            format: options.format,
            uncompressed_size: src_size,
            block_size,
            index_shift: shift,
        };
        let mut header_bytes = [0u8; HEADER_SIZE];
        header.write(&mut header_bytes);
        out_file.write_all(&header_bytes)?;

        let index_bytes: Vec<u8> = index.iter().flat_map(|e| e.to_le_bytes()).collect();
        out_file.write_all(&index_bytes)?;
        // Make sure trailing padding is part of the file even though nothing was
        // written into it.
        out_file.set_len(dst)?;

        Ok(Stats {
            input_size: src_size,
            output_size: dst,
        })
    })();
    finish(&part, output, result)
}

/// Resolve the block size: the format's default, or a requested size that
/// passes maxcso's validation.
fn resolve_block_size(requested: Option<u32>, format: Format, src_size: u64) -> Result<u32> {
    let Some(size) = requested else {
        return Ok(default_block_size(format, src_size));
    };
    if size > crate::format::MAX_BLOCK_SIZE {
        return Err(Error::InvalidOption(format!(
            "block size {size} is larger than {}",
            crate::format::MAX_BLOCK_SIZE
        )));
    }
    if size < SECTOR_SIZE {
        return Err(Error::InvalidOption(
            "block size must be at least 2048".into(),
        ));
    }
    if !size.is_power_of_two() {
        return Err(Error::InvalidOption(
            "block size must be a power of two".into(),
        ));
    }
    Ok(size)
}

fn build_pool(threads: usize) -> Result<ThreadPool> {
    ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|e| Error::InvalidOption(format!("could not start worker threads: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_block_sizes() {
        assert_eq!(default_block_size(Format::Cso, 1 << 30), 8192);
        assert_eq!(default_block_size(Format::Cso, 1 << 31), 16384);
        // Whatever the size: OPL only reads 2 KiB blocks.
        assert_eq!(default_block_size(Format::Zso, 1 << 30), 2048);
        assert_eq!(default_block_size(Format::Zso, 1 << 33), 2048);
    }
}

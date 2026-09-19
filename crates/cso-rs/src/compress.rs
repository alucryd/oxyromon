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

use crate::error::{Error, Result};
use crate::format::{
    align_up, block_count, compute_index_shift, Format, HEADER_SIZE, INDEX_UNCOMPRESSED, SECTOR_SIZE,
};
use crate::io::read_exact_at;
use crate::method::{self, BlockChoice, BlockKind, CostModel, Methods};
use crate::{Progress, Stats};

/// Default block size for ordinary dumps.
pub const DEFAULT_BLOCK_SIZE: u32 = 2048;
/// Above 2 GiB maxcso switches to larger blocks for a better ratio.
pub const LARGE_BLOCK_SIZE: u32 = 16384;
const LARGE_BLOCK_THRESHOLD: u64 = 0x8000_0000;

/// Everything that shapes a CSO/ZSO write.
pub struct CompressOptions {
    pub format: Format,
    /// `None` picks 2048, or 16384 for inputs of 2 GiB or more.
    pub block_size: Option<u32>,
    pub methods: Methods,
    /// 0 means let rayon decide.
    pub threads: usize,
    /// How much a block may grow over its raw size before it is stored raw, as a
    /// percentage of the block size.
    pub orig_max_cost_percent: f64,
    /// How much LZ4 may cost relative to DEFLATE, as a percentage.
    pub lz4_max_cost_percent: f64,
}

impl CompressOptions {
    pub fn new(format: Format) -> CompressOptions {
        CompressOptions {
            format,
            block_size: None,
            methods: Methods::default_for(format),
            threads: 0,
            orig_max_cost_percent: 0.0,
            lz4_max_cost_percent: 0.0,
        }
    }
}

/// Compress a raw ISO into a CSO or ZSO file.
pub fn compress(
    input: &Path,
    output: &Path,
    options: &CompressOptions,
    progress: &mut dyn FnMut(Progress),
) -> Result<Stats> {
    options.methods.validate_for(options.format)?;

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

    let block_size = resolve_block_size(options.block_size, src_size)?;
    let blocks = block_count(src_size, block_size);
    let index_len = (blocks + 1) * 4;
    let unaligned = HEADER_SIZE as u64 + index_len;
    let shift = compute_index_shift(unaligned + src_size);
    let align = 1u64 << shift;
    let data_start = align_up(unaligned, align);
    let cost = CostModel::new(
        options.orig_max_cost_percent,
        options.lz4_max_cost_percent,
        block_size,
    );

    let pool = build_pool(options.threads)?;
    let wave = pool.current_num_threads().max(1) * 4;

    let out_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(output)?;
    let mut writer = BufWriter::with_capacity(256 * 1024, out_file);
    writer.seek(SeekFrom::Start(data_start))?;

    let mut index = vec![0u32; (blocks + 1) as usize];
    let mut dst = data_start;

    for wave_start in (0..blocks).step_by(wave) {
        let wave_end = (wave_start + wave as u64).min(blocks);
        let choices: Vec<BlockChoice> = pool.install(|| {
            (wave_start..wave_end)
                .into_par_iter()
                .map(|i| {
                    let off = i * u64::from(block_size);
                    let want = u64::from(block_size).min(src_size - off) as usize;
                    // The tail is zero-padded to a whole block, as maxcso does.
                    let mut buf = vec![0u8; block_size as usize];
                    read_exact_at(&in_file, &mut buf[..want], off)?;
                    method::compress_block(&buf, options.methods, &cost, align, block_size)
                })
                .collect::<Result<Vec<_>>>()
        })?;

        for (offset_in_wave, choice) in choices.into_iter().enumerate() {
            let i = (wave_start + offset_in_wave as u64) as usize;
            let flag = match choice.kind {
                BlockKind::Orig => INDEX_UNCOMPRESSED,
                _ => 0,
            };
            index[i] = ((dst >> shift) as u32) | flag;

            writer.write_all(&choice.data[..choice.size])?;
            dst += choice.size as u64;

            // Pad to the index alignment. Seek rather than write, so a large
            // shift leaves a sparse hole instead of gigabytes of zeroes.
            let padded = align_up(dst, align);
            if padded > dst {
                writer.seek(SeekFrom::Current((padded - dst) as i64))?;
                dst = padded;
            }

            let done = ((i as u64 + 1) * u64::from(block_size)).min(src_size);
            progress(Progress {
                done,
                total: src_size,
                written: dst,
            });
        }
    }

    index[blocks as usize] = (dst >> shift) as u32;

    let mut out_file = writer
        .into_inner()
        .map_err(|e| Error::Io(e.into_error()))?;
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

    let mut index_bytes = Vec::with_capacity(index_len as usize);
    for entry in &index {
        index_bytes.extend_from_slice(&entry.to_le_bytes());
    }
    out_file.write_all(&index_bytes)?;
    // Make sure trailing padding is part of the file even though nothing was
    // written into it.
    out_file.set_len(dst)?;

    Ok(Stats {
        input_size: src_size,
        output_size: dst,
    })
}

/// Resolve the block size, applying maxcso's size-dependent default and its
/// validation.
pub fn resolve_block_size(requested: Option<u32>, src_size: u64) -> Result<u32> {
    let Some(size) = requested else {
        if src_size >= LARGE_BLOCK_THRESHOLD {
            return Ok(LARGE_BLOCK_SIZE);
        }
        return Ok(DEFAULT_BLOCK_SIZE);
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

//! CSO/ZSO -> ISO.
//!
//! Every block is independently addressable through the index, so blocks are
//! decompressed in parallel waves and written back in order.

use std::ffi::c_int;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::mem;
use std::path::Path;

use rayon::iter::{IntoParallelIterator, ParallelIterator};
use rayon::{ThreadPool, ThreadPoolBuilder};

use crate::Stats;
use crate::error::{Error, Result};
use crate::format::{Format, HEADER_SIZE, Header, INDEX_OFFSET_MASK, INDEX_UNCOMPRESSED};
use crate::io::read_exact_at;
use crate::lz4;

/// Tuning for a decompression run.
#[derive(Debug, Clone, Copy, Default)]
pub struct DecompressOptions {
    /// 0 means let rayon decide.
    pub threads: usize,
}

/// Decompress a CSO or ZSO into the original ISO.
///
/// `progress` is called with each newly consumed chunk of the input, in bytes;
/// for a well-formed file the calls add up to the input file size. On error the
/// partial output file is removed.
pub fn decompress(
    input: &Path,
    output: &Path,
    options: &DecompressOptions,
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    let in_file = File::open(input)?;
    let file_size = in_file.metadata()?.len();
    let header = read_header(&in_file, file_size)?;
    let format = header.format;
    let total = header.uncompressed_size;
    let block_size = header.block_size;
    let align = 1u64 << header.index_shift;
    let blocks = header.block_count();

    let mut index_bytes = vec![0u8; (blocks as usize + 1) * 4];
    read_exact_at(&in_file, &mut index_bytes, HEADER_SIZE as u64)
        .map_err(|e| Error::Corrupt(format!("could not read the block index: {e}")))?;
    let index: Vec<u32> = index_bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .collect();
    let offset = |i: usize| u64::from(index[i] & INDEX_OFFSET_MASK) * align;

    // A block's stored span is the gap between index entries, which includes the
    // alignment padding. Bound it so a corrupt index cannot ask for gigabytes.
    let max_span = u64::from(block_size)
        .saturating_mul(2)
        .max(u64::from(block_size) + align);

    let reader = BlockReader {
        file: &in_file,
        index: &index,
        format,
        align,
        block_size,
        file_size,
        max_span,
    };

    let pool = build_pool(options.threads)?;
    let wave = pool.current_num_threads().max(1) * 4;

    let out_file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(output)?;
    // Everything from here on writes to `output`, so a failure removes it.
    let result = (|| {
        let mut writer = BufWriter::with_capacity(256 * 1024, out_file);
        // The header and index, up to where the first block starts.
        progress(offset(0));

        for wave_start in (0..blocks).step_by(wave) {
            let wave_end = (wave_start + wave as u64).min(blocks);
            let chunks: Vec<Vec<u8>> = pool.install(|| {
                (wave_start..wave_end)
                    .into_par_iter()
                    .map(|i| {
                        let block_off = i * u64::from(block_size);
                        let out_len = u64::from(block_size).min(total - block_off) as usize;
                        reader.read(i, out_len)
                    })
                    .collect::<Result<Vec<_>>>()
            })?;

            for (offset_in_wave, chunk) in chunks.into_iter().enumerate() {
                let i = wave_start + offset_in_wave as u64;
                writer.write_all(&chunk)?;
                // The block's stored span, padding included; `read` validated it.
                progress(offset(i as usize + 1) - offset(i as usize));
            }
        }

        let out_file = writer.into_inner().map_err(|e| Error::Io(e.into_error()))?;
        out_file.set_len(total)?;

        Ok(Stats {
            input_size: file_size,
            output_size: total,
        })
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(output);
    }
    result
}

/// Read and validate just the header.
pub fn read_header(file: &File, file_size: u64) -> Result<Header> {
    if file_size < HEADER_SIZE as u64 {
        return Err(Error::Corrupt("file is shorter than a CSO header".into()));
    }
    let mut buf = [0u8; HEADER_SIZE];
    read_exact_at(file, &mut buf, 0)?;
    let header = Header::parse(&buf)?;

    let needed = header.data_start_unaligned();
    if file_size < needed {
        return Err(Error::Corrupt(format!(
            "file is {file_size} bytes, too short to hold the index which ends at {needed}"
        )));
    }
    Ok(header)
}

/// Everything a single block read needs, bundled so the worker closure stays short.
struct BlockReader<'a> {
    file: &'a File,
    index: &'a [u32],
    format: Format,
    align: u64,
    block_size: u32,
    file_size: u64,
    max_span: u64,
}

impl BlockReader<'_> {
    /// Decompress block `i` into a buffer of exactly `out_len` bytes.
    fn read(&self, i: u64, out_len: usize) -> Result<Vec<u8>> {
        let entry = self.index[i as usize];
        let next = self.index[i as usize + 1];
        let offset = u64::from(entry & INDEX_OFFSET_MASK) * self.align;
        let next_offset = u64::from(next & INDEX_OFFSET_MASK) * self.align;

        if offset > self.file_size {
            return Err(Error::Corrupt(format!(
                "block {i} points at {offset}, past the end of the file"
            )));
        }
        let span = next_offset.checked_sub(offset).ok_or_else(|| {
            Error::Corrupt(format!("block {i} is indexed before the previous block"))
        })?;
        if span > self.max_span {
            return Err(Error::Corrupt(format!(
                "block {i} spans {span} bytes, over the {} byte limit for a {} byte block",
                self.max_span, self.block_size
            )));
        }
        if offset + span > self.file_size {
            return Err(Error::Corrupt(format!(
                "block {i} runs past the end of the file"
            )));
        }

        if entry & INDEX_UNCOMPRESSED != 0 {
            let mut out = vec![0u8; out_len];
            read_exact_at(self.file, &mut out, offset)?;
            return Ok(out);
        }

        let mut raw = vec![0u8; span as usize];
        read_exact_at(self.file, &mut raw, offset)?;
        let mut out = vec![0u8; out_len];
        let got = match self.format.codec() {
            crate::format::Codec::Deflate => inflate_raw(&raw, &mut out)?,
            crate::format::Codec::Lz4 => lz4::decompress_bounded(&raw, &mut out)?,
        };
        if got != out_len {
            return Err(Error::Corrupt(format!(
                "block {i} decompressed to {got} bytes, expected {out_len}"
            )));
        }
        Ok(out)
    }
}

/// Raw DEFLATE inflate that stops when `dst` is full and tolerates the
/// alignment padding that follows a block's stream.
fn inflate_raw(src: &[u8], dst: &mut [u8]) -> Result<usize> {
    unsafe {
        // SAFETY: z_stream holds non-nullable function pointers, so an
        // all-zero one is not a valid Rust value: it stays behind MaybeUninit
        // and is only touched through the raw pointer. Zero is what zlib wants
        // there, though: inflateInit2_ reads zalloc, zfree and opaque, and Z_NULL
        // makes it install its default allocator in their place.
        let mut z = mem::MaybeUninit::<libz_sys::z_stream>::zeroed();
        let zp = z.as_mut_ptr();
        let init = libz_sys::inflateInit2_(
            zp,
            -15,
            libz_sys::zlibVersion(),
            mem::size_of::<libz_sys::z_stream>() as c_int,
        );
        if init != libz_sys::Z_OK {
            return Err(Error::Corrupt(format!(
                "could not initialise the inflate stream ({init})"
            )));
        }

        (*zp).next_in = src.as_ptr() as *mut u8;
        (*zp).avail_in = src.len() as u32;
        (*zp).next_out = dst.as_mut_ptr();
        (*zp).avail_out = dst.len() as u32;

        let ret = libz_sys::inflate(zp, libz_sys::Z_FINISH);
        let written = (*zp).total_out as usize;
        let _ = libz_sys::inflateEnd(zp);

        // Output full: the padding after the stream is not our problem.
        if written == dst.len() {
            return Ok(written);
        }
        if ret == libz_sys::Z_STREAM_END {
            return Ok(written);
        }
        Err(Error::Corrupt(format!(
            "inflate failed with code {ret} after {written} bytes"
        )))
    }
}

fn build_pool(threads: usize) -> Result<ThreadPool> {
    ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|e| Error::InvalidOption(format!("could not start worker threads: {e}")))
}

//! NCZ compression: solid (one zstd stream over the decrypted body) and block
//! (independent zstd frames per block). Mirrors `nsz/SolidCompressor.py` and
//! `nsz/BlockCompressor.py`.
//!
//! The "decrypted body" is the concatenation of the incompressible-header gap
//! (0x4000 .. first section) followed by each section's *decrypted* bytes, in
//! section order — exactly what the decompressor reads back and re-encrypts.
//! It is streamed from a reader so NCAs never have to fit in memory.

use std::io::{self, Read, Seek, SeekFrom, Write};

use rayon::prelude::*;
use zstd::zstd_safe::zstd_sys::ZSTD_EndDirective;
use zstd::zstd_safe::{self, CCtx, CParameter, InBuffer, OutBuffer};

use crate::error::{Error, Result};
use crate::format::ncz::{self, BlockHeader, Section};

/// A `Write` adapter that counts bytes written.
struct Counting<W> {
    inner: W,
    n: u64,
}
impl<W: Write> Write for Counting<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.n += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Solid-compress an NCZ: `header` (0x4000) + section table + one zstd stream
/// over `body`. zstd runs on all available cores. `progress` is called with the
/// number of body bytes compressed so far; zstd buffers input ahead of its
/// workers, so this is measured from the encoder, not from reads. Returns
/// bytes written.
pub fn solid_compress_ncz<R: Read, W: Write>(
    header: &[u8],
    sections: &[Section],
    mut body: R,
    level: i32,
    ldm: bool,
    out: &mut W,
    progress: &mut dyn FnMut(u64),
) -> Result<u64> {
    let mut cw = Counting {
        inner: &mut *out,
        n: 0,
    };
    cw.write_all(header)?;
    cw.write_all(&ncz::write_header(sections, None))?;

    // Driven through zstd-safe rather than `zstd::Encoder` so the context can
    // be asked how much input its workers have actually compressed.
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
    let mut cctx = CCtx::create();
    for param in [
        CParameter::CompressionLevel(level),
        CParameter::EnableLongDistanceMatching(ldm),
        CParameter::NbWorkers(threads),
    ] {
        cctx.set_parameter(param).map_err(zstd_code_err)?;
    }
    let mut input = vec![0u8; 1 << 20];
    let mut output = vec![0u8; CCtx::out_size()];
    loop {
        let n = body.read(&mut input)?;
        let directive = if n == 0 {
            ZSTD_EndDirective::ZSTD_e_end
        } else {
            ZSTD_EndDirective::ZSTD_e_continue
        };
        let mut src = InBuffer::around(&input[..n]);
        loop {
            let mut dst = OutBuffer::around(&mut output[..]);
            let remaining = cctx
                .compress_stream2(&mut dst, &mut src, directive)
                .map_err(zstd_code_err)?;
            cw.write_all(dst.as_slice())?;
            progress(cctx.get_frame_progression().consumed);
            let done = if n == 0 {
                remaining == 0
            } else {
                src.pos() == n
            };
            if done {
                break;
            }
        }
        if n == 0 {
            return Ok(cw.n);
        }
    }
}

/// Block-compress an NCZ: `header` (0x4000) + section table + NCZBLOCK header +
/// independently compressed blocks, compressed in parallel on the rayon pool.
/// A block that doesn't shrink is stored raw. `body_len` must be the exact body
/// length. The block size list is back-patched once all blocks are written, so
/// `out` must be seekable. `progress` is called with the number of body bytes
/// compressed so far. Returns bytes written.
#[allow(clippy::too_many_arguments)]
pub fn block_compress_ncz<R: Read, W: Write + Seek>(
    header: &[u8],
    sections: &[Section],
    mut body: R,
    body_len: u64,
    level: i32,
    ldm: bool,
    block_size_exponent: i8,
    out: &mut W,
    progress: &mut dyn FnMut(u64),
) -> Result<u64> {
    if !(14..=32).contains(&block_size_exponent) {
        return Err(Error::Unsupported(
            "block size exponent must be 14..=32".into(),
        ));
    }
    let block_size = 1u64 << block_size_exponent;
    let block_count = body_len.div_ceil(block_size) as usize;
    let mut bh = BlockHeader {
        version: 2,
        block_type: 1,
        unused: 0,
        block_size_exponent,
        number_of_blocks: block_count as i32,
        decompressed_size: body_len as i64,
        compressed_block_size_list: vec![0; block_count],
    };

    let start = out.stream_position()?;
    out.write_all(header)?;
    let ncz_header = ncz::write_header(sections, Some(&bh));
    out.write_all(&ncz_header)?;
    let size_list_pos = out.stream_position()? - (block_count * 4) as u64;

    // Read a batch of blocks, compress them in parallel, write them in order.
    // ponytail: batch = 2 blocks per thread bounds memory to ~2*threads*block_size.
    let batch = rayon::current_num_threads() * 2;
    let mut remaining = body_len;
    let mut sizes = bh.compressed_block_size_list.iter_mut();
    while remaining > 0 {
        let mut blocks = Vec::with_capacity(batch);
        while blocks.len() < batch && remaining > 0 {
            let len = block_size.min(remaining);
            let mut block = vec![0u8; len as usize];
            body.read_exact(&mut block)?;
            remaining -= len;
            blocks.push(block);
        }
        let compressed: Vec<Vec<u8>> = blocks
            .into_par_iter()
            .map(|block| {
                let c = zstd_compress(&block, level, ldm)?;
                Ok(if c.len() < block.len() { c } else { block })
            })
            .collect::<Result<_>>()?;
        for c in compressed {
            *sizes.next().unwrap() = u32::try_from(c.len())
                .map_err(|_| Error::Unsupported("block larger than 4 GiB".into()))?;
            out.write_all(&c)?;
        }
        progress(body_len - remaining);
    }

    let end = out.stream_position()?;
    out.seek(SeekFrom::Start(size_list_pos))?;
    for s in &bh.compressed_block_size_list {
        out.write_all(&s.to_le_bytes())?;
    }
    out.seek(SeekFrom::Start(end))?;
    Ok(end - start)
}

/// Compress a single buffer with zstd at the given level (+ optional LDM).
fn zstd_compress(data: &[u8], level: i32, ldm: bool) -> Result<Vec<u8>> {
    let mut enc = zstd::Encoder::new(Vec::new(), level).map_err(zstd_err)?;
    enc.long_distance_matching(ldm).map_err(zstd_err)?;
    enc.write_all(data).map_err(zstd_err)?;
    enc.finish().map_err(zstd_err)
}

fn zstd_err(e: io::Error) -> Error {
    Error::Corrupt(format!("zstd: {e}"))
}

fn zstd_code_err(code: usize) -> Error {
    Error::Corrupt(format!("zstd: {}", zstd_safe::get_error_name(code)))
}

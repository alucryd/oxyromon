//! Per-block compression: one encoder per format, and the rule that decides
//! whether a block is worth storing compressed.
//!
//! Each format gets one encoder. A CSO block is zlib's deflate: libdeflate and
//! Zopfli both compress a little better, but an ARK-5 PSP could not load a
//! game compressed with either, while zlib's output loaded fine. A ZSO block is
//! LZ4 HC at level 16, which alone matched every other LZ4 setting combined,
//! decodes as fast as plain LZ4, and plays fine on the same PSP.

use std::ffi::c_int;
use std::mem;

use crate::format::{Format, align_up};
use crate::lz4;

const Z_DEFAULT_STRATEGY: c_int = 0;
const Z_FILTERED: c_int = 1;
const Z_HUFFMAN_ONLY: c_int = 2;
const Z_RLE: c_int = 3;

/// zlib strategies tried on every CSO block, in maxcso's order
/// (`Sector::Compress`). Only a strictly smaller result displaces the best so
/// far, so the order decides which of two equal-sized encodings is kept, and
/// with it that the output is byte-identical to `maxcso --only-zlib`.
const ZLIB_STRATEGIES: [c_int; 4] = [Z_DEFAULT_STRATEGY, Z_FILTERED, Z_HUFFMAN_ONLY, Z_RLE];

/// LZ4 HC level for ZSO blocks: the highest, which no lower level beat.
const LZ4_HC_LEVEL: i32 = 16;

/// One block, as it is stored.
pub struct Block {
    /// Whether `data` is the block itself, stored uncompressed.
    pub raw: bool,
    pub data: Vec<u8>,
}

/// Compress one block for `format`.
///
/// `src` is always exactly one block (zero-padded if it is the tail of the
/// file). The block is stored raw unless compressing it saves space even after
/// padding to `align`, the on-disk alignment implied by `index_shift`:
/// otherwise every read would pay for decompression, for nothing.
pub fn compress_block(src: &[u8], format: Format, align: u64) -> Block {
    let mut scratch = vec![0u8; scratch_size(src.len())];
    let mut best: Option<Vec<u8>> = None;
    match format {
        Format::Cso => {
            for strategy in ZLIB_STRATEGIES {
                if let Some(n) = deflate_raw(src, &mut scratch, strategy)
                    && best.as_ref().is_none_or(|b| n < b.len())
                {
                    best = Some(scratch[..n].to_vec());
                }
            }
        }
        Format::Zso => {
            best = lz4::compress_hc(src, &mut scratch, LZ4_HC_LEVEL).map(|n| scratch[..n].to_vec());
        }
    }
    match best {
        Some(data) if align_up(data.len() as u64, align) < src.len() as u64 => {
            Block { raw: false, data }
        }
        _ => Block {
            raw: true,
            data: src.to_vec(),
        },
    }
}

/// Worst-case scratch either encoder needs.
fn scratch_size(src_len: usize) -> usize {
    // DEFLATE can expand; zlib's documented bound is len + len/1000 + 12,
    // and LZ4's bound is comfortably under that for our block sizes.
    let deflate_bound = src_len + src_len / 1000 + 64;
    deflate_bound.max(lz4::compress_bound(src_len))
}

/// Raw DEFLATE (no zlib wrapper), level 9, at the given strategy -- exactly the
/// stream a CSO block holds.
fn deflate_raw(src: &[u8], dst: &mut [u8], strategy: c_int) -> Option<usize> {
    unsafe {
        // SAFETY: z_stream holds non-nullable function pointers, so an
        // all-zero one is not a valid Rust value: it stays behind MaybeUninit
        // and is only touched through the raw pointer. Zero is what zlib wants
        // there, though: deflateInit2_ reads zalloc, zfree and opaque, and Z_NULL
        // makes it install its default allocator in their place.
        let mut z = mem::MaybeUninit::<libz_sys::z_stream>::zeroed();
        let zp = z.as_mut_ptr();
        let init = libz_sys::deflateInit2_(
            zp,
            9,
            libz_sys::Z_DEFLATED,
            -15,
            9,
            strategy,
            libz_sys::zlibVersion(),
            mem::size_of::<libz_sys::z_stream>() as c_int,
        );
        if init != libz_sys::Z_OK {
            return None;
        }

        (*zp).next_in = src.as_ptr() as *mut u8;
        (*zp).avail_in = src.len() as u32;
        (*zp).next_out = dst.as_mut_ptr();
        (*zp).avail_out = dst.len() as u32;

        let ret = libz_sys::deflate(zp, libz_sys::Z_FINISH);
        let written = (*zp).total_out as usize;
        let _ = libz_sys::deflateEnd(zp);

        if ret == libz_sys::Z_STREAM_END {
            Some(written)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compressible() -> Vec<u8> {
        let mut v = Vec::new();
        while v.len() < 2048 {
            v.extend_from_slice(b"xso-rs compresses blocks of repetitive data. ");
        }
        v.truncate(2048);
        v
    }

    fn noise(len: usize) -> Vec<u8> {
        let mut v = Vec::with_capacity(len);
        let mut state = 0x853C_49E6_748F_EA9Bu64;
        while v.len() < len {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            v.push((state >> 32) as u8);
        }
        v
    }

    #[test]
    fn compressible_blocks_shrink() {
        for format in [Format::Cso, Format::Zso] {
            let block = compress_block(&compressible(), format, 1);
            assert!(!block.raw, "{format:?} stored a compressible block raw");
            assert!(block.data.len() < 2048, "{format:?}: {}", block.data.len());
        }
    }

    #[test]
    fn incompressible_blocks_are_stored_raw() {
        for format in [Format::Cso, Format::Zso] {
            let block = compress_block(&noise(2048), format, 1);
            assert!(block.raw, "{format:?}");
            assert_eq!(block.data, noise(2048));
        }
    }

    #[test]
    fn padding_that_swallows_the_savings_stores_raw() {
        // Aligned to a whole block, even a tiny compressed block takes as much
        // room as the raw one.
        for format in [Format::Cso, Format::Zso] {
            let block = compress_block(&compressible(), format, 2048);
            assert!(block.raw, "{format:?}");
        }
    }
}

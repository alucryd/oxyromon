//! Per-block compression trials and the rule that picks between them.
//!
//! maxcso compresses every block several ways and keeps the smallest result,
//! subject to a cost allowance that lets the uncompressed form win even when a
//! codec produced something marginally smaller (paying CPU on every read to
//! save a few bytes is a bad trade). This module reproduces that selection.

use std::ffi::c_int;
use std::mem;

use crate::error::{Error, Result};
use crate::format::{align_up, Format};
use crate::lz4;

/// Which compressors to try. The set must match the target format's codec:
/// CSO blocks hold DEFLATE, ZSO blocks hold LZ4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Methods {
    /// zlib level 9, default strategy.
    pub zlib: bool,
    /// zlib level 9 with the FILTERED, HUFFMAN_ONLY and RLE strategies.
    pub zlib_brute: bool,
    /// libdeflate level 12.
    pub libdeflate: bool,
    /// Zopfli. Much slower, usually a fraction of a percent smaller.
    pub zopfli: bool,
    /// Fast LZ4.
    pub lz4: bool,
    /// LZ4 HC at level 16.
    pub lz4_hc: bool,
    /// LZ4 HC at levels 4, 7, 10, 13 and 16.
    pub lz4_hc_brute: bool,
}

impl Methods {
    /// maxcso's out-of-the-box set for each format.
    pub fn default_for(format: Format) -> Methods {
        match format {
            // zlib only: libdeflate's output is rejected by some custom
            // firmware, and Zopfli is off because it is slow.
            Format::Cso => Methods {
                zlib: true,
                zlib_brute: true,
                ..Methods::default()
            },
            Format::Zso => Methods {
                lz4: true,
                lz4_hc: true,
                ..Methods::default()
            },
        }
    }

    /// Reject a set that cannot produce the target format's codec, or that mixes
    /// codecs the container cannot hold. A CSO full of LZ4 blocks, or a ZSO full
    /// of DEFLATE, is unreadable by everything downstream.
    pub fn validate_for(self, format: Format) -> Result<()> {
        let deflate = self.zlib || self.zlib_brute || self.libdeflate || self.zopfli;
        let lz4 = self.lz4 || self.lz4_hc || self.lz4_hc_brute;
        match format.codec() {
            crate::format::Codec::Deflate => {
                if lz4 {
                    return Err(Error::InvalidOption(
                "lz4 methods cannot be enabled for CSO: its blocks hold deflate, so an lz4 block would be unreadable"
                    .into(),
            ));
                }
                if !deflate {
                    return Err(Error::InvalidOption(
                        "no deflate method enabled, which is all a CSO can store".into(),
                    ));
                }
            }
            crate::format::Codec::Lz4 => {
                if deflate {
                    return Err(Error::InvalidOption(
                "deflate methods cannot be enabled for ZSO: its blocks hold lz4, so a deflate block would be unreadable"
                    .into(),
            ));
                }
                if !lz4 {
                    return Err(Error::InvalidOption(
                        "no lz4 method enabled, which is all a ZSO can store".into(),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Turn every enabled method into a trial.
    fn trials(self) -> Vec<Trial> {
        let mut t = Vec::new();
        if self.zlib {
            t.push(Trial::Zlib(Z_DEFAULT_STRATEGY));
        }
        if self.zlib_brute {
            t.push(Trial::Zlib(Z_FILTERED));
            t.push(Trial::Zlib(Z_HUFFMAN_ONLY));
            t.push(Trial::Zlib(Z_RLE));
        }
        if self.libdeflate {
            t.push(Trial::Libdeflate);
        }
        if self.zopfli {
            t.push(Trial::Zopfli);
        }
        if self.lz4 {
            t.push(Trial::Lz4);
        }
        if self.lz4_hc {
            t.push(Trial::Lz4Hc(16));
        }
        if self.lz4_hc_brute {
            for level in [4, 7, 10, 13] {
                t.push(Trial::Lz4Hc(level));
            }
        }
        t
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trial {
    Zlib(c_int),
    Libdeflate,
    Zopfli,
    Lz4,
    Lz4Hc(i32),
}

/// What a stored block actually contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    /// Stored as-is.
    Orig,
    Deflate,
    Lz4,
}

/// The winning representation of one block.
pub struct BlockChoice {
    pub kind: BlockKind,
    /// Exactly `size` bytes of payload.
    pub data: Vec<u8>,
    pub size: usize,
}

/// How much larger a form may be before we refuse it.
#[derive(Debug, Clone, Copy)]
pub struct CostModel {
    /// Bytes a block may grow by before we stop calling it uncompressed.
    pub orig_max_cost: usize,
    /// Bytes LZ4 may grow by over a DEFLATE result, and vice versa.
    pub lz4_max_cost: usize,
}

impl CostModel {
    pub fn new(orig_max_cost_percent: f64, lz4_max_cost_percent: f64, block_size: u32) -> CostModel {
        CostModel {
            orig_max_cost: (orig_max_cost_percent * f64::from(block_size) / 100.0) as usize,
            lz4_max_cost: (lz4_max_cost_percent * f64::from(block_size) / 100.0) as usize,
        }
    }

    /// Whether `kind`/`size` beats the current best.
    fn prefers(
        &self,
        kind: BlockKind,
        size: usize,
        best_kind: BlockKind,
        best_size: usize,
    ) -> bool {
        if kind == BlockKind::Lz4 && best_kind == BlockKind::Deflate {
            // LZ4 decompresses much faster, so ties and small losses are its.
            size <= best_size + self.lz4_max_cost
        } else if kind == BlockKind::Deflate && best_kind == BlockKind::Lz4 {
            // The mirror of that.
            size + self.lz4_max_cost < best_size
        } else {
            size + self.orig_max_cost < best_size
        }
    }
}

/// Compress one block, trying every enabled method and keeping the winner.
///
/// `src` is always exactly one block (zero-padded if it is the tail of the
/// file). `align` is the on-disk block alignment implied by `index_shift`; a
/// compressed block that would not be smaller once padded is stored raw.
pub fn compress_block(
    src: &[u8],
    methods: Methods,
    cost: &CostModel,
    align: u64,
    block_size: u32,
) -> Result<BlockChoice> {
    let block_size = block_size as usize;
    let mut best_kind = BlockKind::Orig;
    let mut best_size = block_size;
    let mut best_data: Option<Vec<u8>> = None;

    let scratch_len = scratch_size(src.len());
    let mut scratch = vec![0u8; scratch_len];

    for trial in methods.trials() {
        let Some((kind, size)) = run_trial(trial, src, &mut scratch) else {
            continue;
        };
        if !cost.prefers(kind, size, best_kind, best_size) {
            continue;
        }
        best_kind = kind;
        best_size = size;
        best_data = Some(scratch[..size].to_vec());
    }

    // If padding to the index alignment eats the savings, don't compress: it
    // saves nothing and makes every read pay for decompression.
    if best_kind != BlockKind::Orig
        && align_up(best_size as u64, align) >= u64::from(block_size as u32)
    {
        return Ok(BlockChoice {
            kind: BlockKind::Orig,
            data: src.to_vec(),
            size: block_size,
        });
    }

    Ok(match best_data {
        Some(data) => BlockChoice {
            kind: best_kind,
            data,
            size: best_size,
        },
        None => BlockChoice {
            kind: BlockKind::Orig,
            data: src.to_vec(),
            size: block_size,
        },
    })
}

/// Worst-case scratch a single trial needs.
fn scratch_size(src_len: usize) -> usize {
    // DEFLATE can expand; zlib's documented bound is len + len/1000 + 12,
    // and LZ4's bound is comfortably under that for our block sizes.
    let deflate_bound = src_len + src_len / 1000 + 64;
    deflate_bound.max(lz4::compress_bound(src_len))
}

fn run_trial(trial: Trial, src: &[u8], scratch: &mut [u8]) -> Option<(BlockKind, usize)> {
    match trial {
        Trial::Zlib(strategy) => deflate_raw(src, scratch, strategy).map(|n| (BlockKind::Deflate, n)),
        Trial::Libdeflate => libdeflate_raw(src, scratch).map(|n| (BlockKind::Deflate, n)),
        Trial::Zopfli => zopfli_raw(src).and_then(|v| {
            if v.len() > scratch.len() {
                return None;
            }
            scratch[..v.len()].copy_from_slice(&v);
            Some((BlockKind::Deflate, v.len()))
        }),
        Trial::Lz4 => lz4::compress_default(src, scratch).map(|n| (BlockKind::Lz4, n)),
        Trial::Lz4Hc(level) => lz4::compress_hc(src, scratch, level).map(|n| (BlockKind::Lz4, n)),
    }
}

const Z_DEFAULT_STRATEGY: c_int = 0;
const Z_FILTERED: c_int = 1;
const Z_HUFFMAN_ONLY: c_int = 2;
const Z_RLE: c_int = 3;

/// Raw DEFLATE (no zlib wrapper), level 9, at the given strategy -- exactly the
/// stream a CSO block holds.
fn deflate_raw(src: &[u8], dst: &mut [u8], strategy: c_int) -> Option<usize> {
    unsafe {
        // SAFETY: z_stream contains non-null function pointers, so a zeroed
        // instance is not a valid z_stream on its own. We use MaybeUninit to
        // avoid creating an invalid value in safe Rust, then immediately pass
        // the pointer to deflateInit2_, which overwrites every field before
        // any read occurs.
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

fn libdeflate_raw(src: &[u8], dst: &mut [u8]) -> Option<usize> {
    let mut compressor = libdeflater::Compressor::new(libdeflater::CompressionLvl::new(12).ok()?);
    compressor.deflate_compress(src, dst).ok()
}

#[cfg(feature = "zopfli")]
fn zopfli_raw(src: &[u8]) -> Option<Vec<u8>> {
    use std::num::NonZeroU64;
    let options = zopfli::Options {
        // maxcso's setting: 5 passes is the sane ceiling above a few MB.
        iteration_count: NonZeroU64::new(5)?,
        ..zopfli::Options::default()
    };
    let mut out = Vec::new();
    zopfli::compress(options, zopfli::Format::Deflate, src, &mut out).ok()?;
    Some(out)
}

#[cfg(not(feature = "zopfli"))]
fn zopfli_raw(_src: &[u8]) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compressible() -> Vec<u8> {
        let mut v = Vec::new();
        while v.len() < 2048 {
            v.extend_from_slice(b"cso-rs compresses blocks of repetitive data. ");
        }
        v.truncate(2048);
        v
    }

    #[test]
    fn methods_must_match_the_container() {
        assert!(Methods::default_for(Format::Cso)
            .validate_for(Format::Cso)
            .is_ok());
        assert!(Methods::default_for(Format::Zso)
            .validate_for(Format::Zso)
            .is_ok());
        // Nothing enabled that the container can hold.
        assert!(Methods::default().validate_for(Format::Cso).is_err());
        assert!(Methods::default().validate_for(Format::Zso).is_err());
        // The wrong codec is rejected outright, even alongside a valid one: a CSO
        // with an lz4 block in it is unreadable, not merely suboptimal.
        let mixed = Methods {
            zlib: true,
            lz4: true,
            ..Methods::default()
        };
        assert!(mixed.validate_for(Format::Cso).is_err());
        let mixed = Methods {
            lz4: true,
            libdeflate: true,
            ..Methods::default()
        };
        assert!(mixed.validate_for(Format::Zso).is_err());
    }

    #[test]
    fn deflate_trials_actually_shrink() {
        let src = compressible();
        let cost = CostModel::new(0.0, 0.0, 2048);
        let methods = Methods {
            zlib: true,
            zlib_brute: true,
            libdeflate: true,
            ..Methods::default()
        };
        let choice = compress_block(&src, methods, &cost, 1, 2048).unwrap();
        assert_eq!(choice.kind, BlockKind::Deflate);
        assert!(choice.size < 2048, "expected shrinkage, got {}", choice.size);
        assert_eq!(choice.data.len(), choice.size);
    }

    #[test]
    fn incompressible_blocks_are_stored_raw() {
        let cost = CostModel::new(0.0, 0.0, 2048);
        let methods = Methods {
            zlib: true,
            zlib_brute: true,
            libdeflate: true,
            lz4: true,
            lz4_hc: true,
            ..Methods::default()
        };
        let choice = compress_block(&noise(2048), methods, &cost, 1, 2048).unwrap();
        assert_eq!(choice.kind, BlockKind::Orig);
        assert_eq!(choice.size, 2048);
        assert_eq!(choice.data, noise(2048));
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
    fn orig_cost_allows_a_little_growth_before_giving_up() {
        // With a generous orig_max_cost, a barely-smaller result still counts as
        // compressed; with none, the same input is judged the same way but the
        // threshold differs by exactly the allowance.
        let cost = CostModel::new(10.0, 0.0, 2048);
        assert_eq!(cost.orig_max_cost, 204);
        assert!(cost.prefers(BlockKind::Deflate, 1843, BlockKind::Orig, 2048));
        assert!(!cost.prefers(BlockKind::Deflate, 1844, BlockKind::Orig, 2048));
    }

    #[test]
    fn lz4_wins_ties_against_deflate() {
        // Costs are byte allowances; CostModel::new derives them from a
        // percentage of the block size.
        let cost = CostModel {
            orig_max_cost: 0,
            lz4_max_cost: 10,
        };
        assert!(cost.prefers(BlockKind::Lz4, 1000, BlockKind::Deflate, 1000));
        assert!(cost.prefers(BlockKind::Lz4, 1010, BlockKind::Deflate, 1000));
        assert!(!cost.prefers(BlockKind::Lz4, 1011, BlockKind::Deflate, 1000));
        // Deflate needs a real margin to take the block back from LZ4.
        assert!(!cost.prefers(BlockKind::Deflate, 1010, BlockKind::Lz4, 1000));
        assert!(cost.prefers(BlockKind::Deflate, 989, BlockKind::Lz4, 1000));
    }

    #[test]
    fn costs_are_percentages_of_the_block_size() {
        let cost = CostModel::new(2.5, 1.0, 2048);
        assert_eq!(cost.orig_max_cost, 51);
        assert_eq!(cost.lz4_max_cost, 20);
    }
}


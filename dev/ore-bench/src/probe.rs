//! Questions the profile in `formats/design/07-gamecube-wii-profile.md` left
//! open, each answered on a real disc:
//!
//! - [`wii`]: how many of a Wii disc's stored hashes recomputing would not
//!   give back (the hash exceptions an ORE must keep), and what the bytes
//!   outside every partition are: zeros, padding, or data;
//! - [`speed`]: how fast groups of deduped chunks compress and decompress,
//!   one core at a time, by size and codec: the read cost of a group.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Instant;

use anyhow::{Result, bail};
use fastcdc::v2020::FastCDC;
use gcwii::disc::{PARTITION_DATA_SIZE, Source, Volume};
use gcwii::wii::{
    self, BLOCK_DATA_SIZE, BLOCK_TOTAL_SIZE, BLOCKS_PER_GROUP, BlockData, DIGEST_SIZE, H0, H1, H2,
    HashBlock,
};
use rayon::prelude::*;
use sha1::{Digest, Sha1};

use super::dedup::for_each_area;
use super::{RvzDisc, find_filler};

/// What [`wii`] finds on a disc.
#[derive(Debug, Default)]
pub struct WiiProbe {
    pub partitions: u64,
    pub blocks: u64,
    /// Stored digests, of H0, H1 and H2, recomputing does not give back.
    pub exceptions: u64,
    /// Blocks holding at least one.
    pub blocks_with_exceptions: u64,
    /// Bytes outside every partition and the header area before the first.
    pub outside: u64,
    pub outside_zero: u64,
    pub outside_padding: u64,
    /// The rest, and what Zstandard 19 makes of it.
    pub outside_other: u64,
    pub outside_other_compressed: u64,
}

/// Probe a Wii disc read from an RVZ, which gives back the disc's own
/// hashes, exceptions included.
pub fn wii(path: &Path) -> Result<WiiProbe> {
    let mut disc = RvzDisc::open(path)?;
    let Some(mut volume) = Volume::new(&mut disc, &[])? else {
        bail!("neither a GameCube nor a Wii disc");
    };
    let mut probe = WiiProbe::default();
    let mut covered: Vec<(u64, u64)> = vec![(0, volume.partitions.first().copied().unwrap_or(0))];
    for &partition in &volume.partitions.clone() {
        probe.partitions += 1;
        let data_offset = partition + volume.data_offset(&mut disc, partition);
        let raw_size = volume.read_shifted(&mut disc, partition + PARTITION_DATA_SIZE, None)?;
        covered.push((partition, data_offset + raw_size));
        let key = volume.key(&mut disc, partition)?;
        let blocks = raw_size / BLOCK_TOTAL_SIZE;
        probe.blocks += blocks;
        let mut raw = vec![0; (BLOCKS_PER_GROUP * BLOCK_TOTAL_SIZE) as usize];
        for group in 0..blocks.div_ceil(BLOCKS_PER_GROUP) {
            let in_group = (blocks - group * BLOCKS_PER_GROUP).min(BLOCKS_PER_GROUP) as usize;
            let raw = &mut raw[..in_group * BLOCK_TOTAL_SIZE as usize];
            disc.read_at(
                data_offset + group * BLOCKS_PER_GROUP * BLOCK_TOTAL_SIZE,
                raw,
            )?;
            let (stored, data): (Vec<HashBlock>, Vec<BlockData>) = raw
                .par_chunks(BLOCK_TOTAL_SIZE as usize)
                .map(|block| {
                    let mut data = [0; BLOCK_DATA_SIZE as usize];
                    wii::decrypt_data(&key, block, &mut data);
                    (wii::decrypt_hashes(&key, block), data)
                })
                .unzip();
            let mut padded = data;
            padded.resize(BLOCKS_PER_GROUP as usize, [0; BLOCK_DATA_SIZE as usize]);
            let computed = wii::hash_group(&padded);
            for (stored, computed) in stored.iter().zip(&computed) {
                let mut differ = 0;
                for (start, length) in [H0, H1, H2] {
                    for at in (start..start + length).step_by(DIGEST_SIZE) {
                        if stored[at..at + DIGEST_SIZE] != computed[at..at + DIGEST_SIZE] {
                            differ += 1;
                        }
                    }
                }
                probe.exceptions += differ;
                probe.blocks_with_exceptions += (differ > 0) as u64;
            }
        }
    }

    // What lies outside: between and after the partitions
    covered.sort_unstable();
    let size = disc.size();
    let mut at = 0;
    let mut gaps = Vec::new();
    for (start, end) in covered {
        if start > at {
            gaps.push((at, start));
        }
        at = at.max(end);
    }
    if size > at {
        gaps.push((at, size));
    }
    for (start, end) in gaps {
        let mut data = vec![0; (end - start) as usize];
        disc.read_at(start, &mut data)?;
        probe.outside += data.len() as u64;
        // Padding is found relative to the disc, so from a block boundary
        let lead = (start % 0x8000) as usize;
        let mut aligned = vec![0; lead];
        aligned.extend_from_slice(&data);
        let padding = find_filler(&aligned, None);
        // Zeros and the rest, outside the padding runs
        let mut other = Vec::new();
        let mut zeros = 0u64;
        let mut rest = |segment: &[u8], other: &mut Vec<u8>| {
            zeros += segment.iter().filter(|&&byte| byte == 0).count() as u64;
            other.extend(segment.iter().filter(|&&byte| byte != 0));
        };
        let mut position = lead;
        for (run_start, run_end) in padding {
            probe.outside_padding += run_end - run_start;
            rest(&aligned[position..run_start as usize], &mut other);
            position = run_end as usize;
        }
        rest(&aligned[position..], &mut other);
        probe.outside_zero += zeros;
        probe.outside_other += other.len() as u64;
        probe.outside_other_compressed += other
            .par_chunks(2 << 20)
            .map(|piece| Ok(zstd::bulk::compress(piece, 19)?.len() as u64))
            .sum::<Result<u64>>()?;
    }
    Ok(probe)
}

/// One measure of [`speed`]: a group size and codec, its ratio, and its
/// speed on one core.
#[derive(Debug)]
pub struct Speed {
    pub group: usize,
    pub codec: &'static str,
    pub groups: usize,
    pub input: u64,
    pub output: u64,
    pub compress_seconds: f64,
    pub decompress_seconds: f64,
}

fn compress(group: &[u8], codec: &str) -> Result<Vec<u8>> {
    Ok(match codec {
        "zstd19" => zstd::bulk::compress(group, 19)?,
        "lzma2" => {
            let mut options = lzma_rust2::Lzma2Options::with_preset(9);
            options.lzma_options.dict_size = dict_size(group.len());
            let mut writer = lzma_rust2::Lzma2Writer::new(Vec::new(), options);
            writer.write_all(group)?;
            writer.finish()?
        }
        _ => bail!("unknown codec: {codec}"),
    })
}

fn decompress(compressed: &[u8], length: usize, codec: &str) -> Result<Vec<u8>> {
    Ok(match codec {
        "zstd19" => zstd::bulk::decompress(compressed, length)?,
        "lzma2" => {
            let mut out = Vec::with_capacity(length);
            lzma_rust2::Lzma2Reader::new(compressed, dict_size(length), None)
                .read_to_end(&mut out)?;
            out
        }
        _ => bail!("unknown codec: {codec}"),
    })
}

fn dict_size(length: usize) -> u32 {
    (length.next_power_of_two() as u32).clamp(4096, 64 << 20)
}

/// Compress and decompress, one group at a time on one core, the groups of
/// `sample` bytes of a disc's deduped 16 KiB chunks, taken evenly across it,
/// checking each comes back whole.
pub fn speed(
    path: &Path,
    sample: u64,
    sizes: &[usize],
    codecs: &[&'static str],
) -> Result<Vec<Speed>> {
    // The disc's stored chunks, deduped, in the order first met
    let mut seen: HashSet<[u8; 20]> = HashSet::new();
    let mut stored: Vec<u8> = Vec::new();
    let mut total = 0u64;
    let mut areas = Vec::new();
    for_each_area(path, &mut |area| {
        total += area.len() as u64;
        areas.push(area);
        Ok(())
    })?;
    let every = total.div_ceil(sample).max(1);
    for area in &areas {
        for (index, chunk) in FastCDC::new(area, 4 << 10, 16 << 10, 64 << 10).enumerate() {
            let bytes = &area[chunk.offset..chunk.offset + chunk.length];
            // Runs of 256 chunks, about 4 MiB, so that groups are of chunks
            // that lie together, as they are stored
            if (index as u64 / 256).is_multiple_of(every) && seen.insert(Sha1::digest(bytes).into())
            {
                stored.extend_from_slice(bytes);
            }
        }
    }
    drop(areas);
    let mut speeds = Vec::new();
    for &codec in codecs {
        for &size in sizes {
            let groups: Vec<&[u8]> = stored
                .chunks(size)
                .filter(|group| group.len() == size)
                .collect();
            let (mut output, mut compress_seconds, mut decompress_seconds) = (0u64, 0f64, 0f64);
            for group in &groups {
                let start = Instant::now();
                let compressed = compress(group, codec)?;
                compress_seconds += start.elapsed().as_secs_f64();
                let start = Instant::now();
                let back = decompress(&compressed, group.len(), codec)?;
                decompress_seconds += start.elapsed().as_secs_f64();
                if back != *group {
                    bail!("{codec} did not give a {size}-byte group back");
                }
                output += compressed.len() as u64;
            }
            speeds.push(Speed {
                group: size,
                codec,
                groups: groups.len(),
                input: (groups.len() * size) as u64,
                output,
                compress_seconds,
                decompress_seconds,
            });
        }
    }
    Ok(speeds)
}

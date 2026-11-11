//! Content-defined chunking algorithms, and the order stored chunks are
//! grouped in: the two questions `formats/design/07-gamecube-wii-profile.md`
//! left after settling on FastCDC and groups in the order first met.
//!
//! [`compare`] cuts every area of a set of discs with every [`Chunker`],
//! dedups each across the whole set, and compresses what each stores in
//! 1 MiB Zstandard groups, as the `stream` profile does. Chunkers differ in
//! the average chunk they produce, and smaller chunks dedup more, so each is
//! to be read against FastCDC at several averages.
//!
//! [`order`] takes one chunker, FastCDC at 16 KiB, and groups what a disc
//! stores in three orders: as first met, and sorted by two sketches of each
//! chunk's content, so that chunks alike share a group.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

use anyhow::Result;
use cdc_chunkers::{SizeParams, leap_based, rabin, seq, supercdc, ultra};
use fastcdc::v2020::FastCDC;
use mothcdc::MothChunker;
use mothcdc::mincdc::{MinCdcHash4, SliceChunker};
use rayon::prelude::*;
use sha1::{Digest, Sha1};

use super::dedup::for_each_area;

const LEVEL: i32 = 19;
/// The index, as the other runs count it: 8 bytes a chunk referenced, 28 a
/// chunk stored, 16 a group.
const REFERENCE: u64 = 8;
const STORED: u64 = 28;
const GROUP: u64 = 16;

/// A chunking algorithm and the average it is asked for.
#[derive(Clone, Copy, Debug)]
pub enum Chunker {
    /// FastCDC 2020, from N/4 to 4N.
    FastCdc(usize),
    /// MinCDC (MinCdcHash4), from N/2 to 3N/2, the narrow range its authors
    /// recommend; its chunks are spread evenly over it.
    MinCdc(usize),
    /// MothCDC: MinCDC's chunks, a run of identical ones one record.
    MothCdc(usize),
    /// SeqCDC, increasing mode, from N/2 to 2N.
    SeqCdc(usize),
    /// UltraCDC, from N/4 to 4N.
    UltraCdc(usize),
    /// SuperCDC, from N/4 to 4N.
    SuperCdc(usize),
    /// Leap-based CDC, from N/4 to 4N.
    LeapCdc(usize),
    /// Rabin, from N/2 to 2N.
    Rabin(usize),
    /// QuickCDC, its maximum 4N.
    QuickCdc(usize),
}

impl Chunker {
    pub fn name(&self) -> String {
        let (name, n) = match *self {
            Self::FastCdc(n) => ("fastcdc", n),
            Self::MinCdc(n) => ("mincdc", n),
            Self::MothCdc(n) => ("mothcdc", n),
            Self::SeqCdc(n) => ("seqcdc", n),
            Self::UltraCdc(n) => ("ultracdc", n),
            Self::SuperCdc(n) => ("supercdc", n),
            Self::LeapCdc(n) => ("leapcdc", n),
            Self::Rabin(n) => ("rabin", n),
            Self::QuickCdc(n) => ("quickcdc", n),
        };
        format!("{name}-{}K", n >> 10)
    }

    /// Each chunk of `data` as its offset and length, and how many records
    /// describe them: one a chunk, but for MothCDC's runs.
    pub fn chunks(&self, data: &[u8]) -> (Vec<(usize, usize)>, u64) {
        let sizes = |n: usize, min: usize, max: usize| SizeParams::new(min, n, max);
        let chunks: Vec<(usize, usize)> = match *self {
            Self::FastCdc(n) => FastCDC::new(data, n / 4, n, n * 4)
                .map(|chunk| (chunk.offset, chunk.length))
                .collect(),
            Self::MinCdc(n) => SliceChunker::new(data, n / 2, n * 3 / 2, MinCdcHash4::new())
                .map(|chunk| (chunk.offset() as usize, chunk.len()))
                .collect(),
            Self::MothCdc(n) => {
                let mut chunks = Vec::new();
                let mut records = 0;
                for segment in MothChunker::new(data, n / 2, n * 3 / 2) {
                    records += 1;
                    let (offset, length) = (segment.offset() as usize, segment.len() as usize);
                    let count = segment.chunk_count() as usize;
                    let each = length / count;
                    for i in 0..count {
                        chunks.push((offset + i * each, each));
                    }
                }
                return (chunks, records);
            }
            Self::SeqCdc(n) => seq::Chunker::new(
                data,
                sizes(n, n / 2, n * 2),
                seq::OperationMode::Increasing,
                seq::Config::default(),
            )
            .map(|chunk| (chunk.pos, chunk.len))
            .collect(),
            Self::UltraCdc(n) => ultra::Chunker::new(data, sizes(n, n / 4, n * 4))
                .map(|chunk| (chunk.pos, chunk.len))
                .collect(),
            Self::SuperCdc(n) => {
                supercdc::Chunker::with_records(data, HashMap::new(), sizes(n, n / 4, n * 4))
                    .map(|chunk| (chunk.pos, chunk.len))
                    .collect()
            }
            Self::LeapCdc(n) => leap_based::Chunker::new(data, sizes(n, n / 4, n * 4))
                .map(|chunk| (chunk.pos, chunk.len))
                .collect(),
            Self::Rabin(n) => rabin::Chunker::with_params(
                data,
                rabin::ChunkerParams::new(),
                sizes(n, n / 2, n * 2),
            )
            .map(|chunk| (chunk.pos, chunk.len))
            .collect(),
            Self::QuickCdc(n) => {
                let base = data.as_ptr() as usize;
                quickcdc::Chunker::with_params(data, n, n * 4, 0x9E37_79B9_7F4A_7C15)
                    .expect("valid QuickCDC sizes")
                    .map(|chunk| (chunk.as_ptr() as usize - base, chunk.len()))
                    .collect()
            }
        };
        let records = chunks.len() as u64;
        (chunks, records)
    }
}

/// What [`compare`] finds for one chunker over the whole set.
#[derive(Debug, Default)]
pub struct Comparison {
    pub bytes: u64,
    pub chunks: u64,
    pub records: u64,
    pub stored_chunks: u64,
    pub stored_bytes: u64,
    /// Compressed in 1 MiB Zstandard groups, index included.
    pub compressed: u64,
    /// Chunking alone, one core, summed over areas.
    pub chunk_seconds: f64,
}

fn compress_groups(stored: &[&[u8]], group: usize) -> Result<(u64, u64)> {
    let mut groups: Vec<Vec<u8>> = vec![Vec::new()];
    for chunk in stored {
        if groups.last().is_some_and(|last| last.len() >= group) {
            groups.push(Vec::new());
        }
        groups.last_mut().expect("a group").extend_from_slice(chunk);
    }
    groups.retain(|group| !group.is_empty());
    let size = groups
        .par_iter()
        .map(|group| Ok((zstd::bulk::compress(group, LEVEL)?.len()).min(group.len()) as u64))
        .sum::<Result<u64>>()?;
    Ok((size, groups.len() as u64))
}

/// Every chunker over `discs`, deduped across them all.
pub fn compare(discs: &[&Path], chunkers: &[Chunker]) -> Result<Vec<Comparison>> {
    let mut results: Vec<Comparison> = chunkers.iter().map(|_| Comparison::default()).collect();
    let mut seen: Vec<HashSet<[u8; 20]>> = chunkers.iter().map(|_| HashSet::new()).collect();
    for disc in discs {
        eprintln!("{}", disc.display());
        for_each_area(disc, &mut |area| {
            for ((chunker, result), seen) in chunkers.iter().zip(&mut results).zip(&mut seen) {
                let start = Instant::now();
                let (chunks, records) = chunker.chunks(&area);
                result.chunk_seconds += start.elapsed().as_secs_f64();
                let digests: Vec<[u8; 20]> = chunks
                    .par_iter()
                    .map(|&(offset, length)| Sha1::digest(&area[offset..offset + length]).into())
                    .collect();
                let mut stored = Vec::new();
                for (&(offset, length), digest) in chunks.iter().zip(digests) {
                    if seen.insert(digest) {
                        stored.push(&area[offset..offset + length]);
                    }
                }
                let (compressed, groups) = compress_groups(&stored, 1 << 20)?;
                result.bytes += area.len() as u64;
                result.chunks += chunks.len() as u64;
                result.records += records;
                result.stored_chunks += stored.len() as u64;
                result.stored_bytes += stored.iter().map(|chunk| chunk.len() as u64).sum::<u64>();
                result.compressed += compressed
                    + records * REFERENCE
                    + stored.len() as u64 * STORED
                    + groups * GROUP;
            }
            Ok(())
        })?;
    }
    Ok(results)
}

/// The orders [`order`] groups stored chunks in.
pub const ORDERS: &[&str] = &["first-met", "minhash", "superfeature"];

/// A chunk's 64-bit gear-style hash at every 8-byte window, as sketches use.
fn window_hashes(chunk: &[u8]) -> impl Iterator<Item = u64> + '_ {
    chunk.windows(8).map(|window| {
        u64::from_le_bytes(window.try_into().expect("8 bytes"))
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .rotate_left(29)
            .wrapping_mul(0xBF58_476D_1CE4_E5B9)
    })
}

/// The sort key a chunk is grouped by in `order`: `minhash`, the least of
/// its window hashes, which chunks sharing much content tend to share; or
/// `superfeature`, Finesse-style, the greatest hash in each quarter of the
/// chunk, hashed together, which near-identical chunks share.
fn sketch(chunk: &[u8], order: &str) -> u64 {
    match order {
        "minhash" => window_hashes(chunk).min().unwrap_or(0),
        _ => {
            let quarter = chunk.len().div_ceil(4).max(8);
            let mut combined = 0u64;
            for part in chunk.chunks(quarter) {
                let feature = window_hashes(part).max().unwrap_or(0);
                combined = (combined ^ feature).wrapping_mul(0x94D0_49BB_1331_11EB);
            }
            combined
        }
    }
}

/// What each order weighs, by group size and codec, over `discs`, each disc
/// deduped and ordered alone, as an encoder groups what one disc stores.
/// Returns, per order, the total compressed in 1 MiB and 4 MiB Zstandard
/// groups.
pub fn order(discs: &[&Path]) -> Result<Vec<(&'static str, u64, u64)>> {
    let mut totals: Vec<(u64, u64)> = ORDERS.iter().map(|_| (0, 0)).collect();
    for disc in discs {
        eprintln!("{}", disc.display());
        let mut seen: HashSet<[u8; 20]> = HashSet::new();
        for_each_area(disc, &mut |area| {
            let chunks: Vec<&[u8]> = FastCDC::new(&area, 4 << 10, 16 << 10, 64 << 10)
                .map(|chunk| &area[chunk.offset..chunk.offset + chunk.length])
                .collect();
            let mut stored: Vec<(usize, &[u8])> = Vec::new();
            for chunk in &chunks {
                if seen.insert(Sha1::digest(chunk).into()) {
                    stored.push((stored.len(), chunk));
                }
            }
            for (&order, total) in ORDERS.iter().zip(&mut totals) {
                let mut keyed: Vec<(u64, usize, &[u8])> = stored
                    .par_iter()
                    .map(|&(index, chunk)| {
                        let key = if order == "first-met" {
                            0
                        } else {
                            sketch(chunk, order)
                        };
                        (key, index, chunk)
                    })
                    .collect();
                keyed.sort_unstable_by_key(|&(key, index, _)| (key, index));
                let sorted: Vec<&[u8]> = keyed.into_iter().map(|(_, _, chunk)| chunk).collect();
                let index = chunks.len() as u64 * REFERENCE + stored.len() as u64 * STORED;
                let (one, groups_one) = compress_groups(&sorted, 1 << 20)?;
                let (four, groups_four) = compress_groups(&sorted, 4 << 20)?;
                total.0 += one + index + groups_one * GROUP;
                total.1 += four + index + groups_four * GROUP;
            }
            Ok(())
        })?;
    }
    Ok(ORDERS
        .iter()
        .zip(totals)
        .map(|(&order, (one, four))| (order, one, four))
        .collect())
}

//! B5 for GameCube discs ([formats/benchmark-protocol.md]): how far ORE's
//! levers take chunked compression, against RVZ at the same codec, level and
//! chunk size.
//!
//! RVZ is measured by writing one with rvz-rs, which is Dolphin's byte for
//! byte with Zstandard. The levers are modelled rather than written, as no
//! container exists yet: each chunk is compressed on its own, as in RVZ, and
//! its compressed size counted, plus 8 bytes of index a chunk, as RVZ's group
//! entries take. Header and metadata are left out on the ORE side, so the
//! model is a lower bound on what a container would weigh. Each lever adds
//! to the ones before it:
//!
//! - `plain`: every chunk compressed, nothing else.
//! - `dedup`: identical chunks stored once, as RVZ does.
//! - `filler`: the padding the disc's generator produced cut out of each
//!   chunk and stored as its 68-byte seed, found as RVZ finds it.
//! - `bcj`: the boot DOL's PowerPC branches made absolute before compressing,
//!   which RVZ does not do.
//!
//! [formats/benchmark-protocol.md]: ../../formats/benchmark-protocol.md

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use gcwii::disc::{Platform, Source, Volume};
use gcwii::fst::FileSystem;
use gcwii::lfg::{Lfg, SEED_BYTES, SEED_SIZE};
use lzma_rust2::filter::bcj::BcjWriter;
use rayon::prelude::*;
use sha1::{Digest, Sha1};

pub mod cdc;
pub mod dedup;
pub mod dict;
pub mod inventory;
pub mod probe;
pub mod solid;

/// The span the generator restarts at, and RVZ looks for padding in.
const BLOCK: u64 = 0x8000;
/// Index bytes a chunk, as an RVZ group entry.
const INDEX_ENTRY: u64 = 8;

/// A disc held in memory.
pub struct Disc(pub Vec<u8>);

impl Source for Disc {
    fn size(&self) -> u64 {
        self.0.len() as u64
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> gcwii::error::Result<()> {
        let start = offset as usize;
        let bytes = self
            .0
            .get(start..start + buf.len())
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::UnexpectedEof))?;
        buf.copy_from_slice(bytes);
        Ok(())
    }
}

impl Disc {
    /// Read a disc whole, from an RVZ or a raw image.
    pub fn load(path: &Path) -> Result<Self> {
        if rvz_rs::is_rvz(path)? {
            let mut reader = rvz_rs::Reader::open(path)?;
            let mut data = vec![0; reader.size() as usize];
            for (i, piece) in data.chunks_mut(64 << 20).enumerate() {
                reader.read_at((i as u64) << 26, piece)?;
            }
            Ok(Self(data))
        } else {
            Ok(Self(std::fs::read(path)?))
        }
    }
}

/// An RVZ read through a window of 64 MiB, so that reading its disc in
/// order, a block at a time, decodes each chunk of it once.
pub struct RvzDisc {
    reader: rvz_rs::Reader,
    start: u64,
    window: Vec<u8>,
}

impl RvzDisc {
    const WINDOW: u64 = 64 << 20;

    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self {
            reader: rvz_rs::Reader::open(path)?,
            start: 0,
            window: Vec::new(),
        })
    }
}

impl Source for RvzDisc {
    fn size(&self) -> u64 {
        self.reader.size()
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> gcwii::error::Result<()> {
        let io = |error: rvz_rs::Error| std::io::Error::other(error.to_string());
        let end = offset + buf.len() as u64;
        if offset < self.start || end > self.start + self.window.len() as u64 {
            if buf.len() as u64 > Self::WINDOW {
                return Ok(self.reader.read_at(offset, buf).map_err(io)?);
            }
            self.start = offset / Self::WINDOW * Self::WINDOW;
            let length = (2 * Self::WINDOW).min(self.reader.size().saturating_sub(self.start));
            self.window.resize(length as usize, 0);
            self.reader
                .read_at(self.start, &mut self.window)
                .map_err(io)?;
        }
        let at = (offset - self.start) as usize;
        buf.copy_from_slice(&self.window[at..at + buf.len()]);
        Ok(())
    }

    fn title_key(&self, data_offset: u64) -> Option<gcwii::crypto::Key> {
        self.reader
            .title_keys()
            .find(|&(offset, _)| offset == data_offset)
            .map(|(_, key)| key)
    }
}

/// What the levers act on, found once a disc.
pub struct Layout {
    /// Runs of padding as `[start, end)`, none crossing a block.
    pub filler: Vec<(u64, u64)>,
    /// The boot DOL, as `[start, end)`.
    pub dol: Option<(u64, u64)>,
}

impl Layout {
    pub fn of(disc: &mut Disc) -> Result<Self> {
        let Some(mut volume) = Volume::new(disc, &[])? else {
            bail!("neither a GameCube nor a Wii disc");
        };
        if volume.platform != Platform::GameCube {
            bail!("a Wii disc: this bench covers GameCube discs only");
        }
        let file_system = volume.file_system(disc, None)?;
        let dol = match volume.dol_offset(disc, None)? {
            Some(offset) => {
                let size = volume.dol_size(disc, None, offset)?;
                Some((offset, (offset + size as u64).min(disc.size())))
            }
            None => None,
        };
        Ok(Self {
            filler: find_filler(&disc.0, file_system.as_deref()),
            dol,
        })
    }

    pub fn filler_bytes(&self) -> u64 {
        self.filler.iter().map(|(start, end)| end - start).sum()
    }
}

/// The padding of a disc, found as rvz-rs's `rvz_pack` finds it: past the
/// zeros that follow a file, the generator's output to the end of the block,
/// or to the end of the file that follows it.
pub(crate) fn find_filler(data: &[u8], file_system: Option<&FileSystem>) -> Vec<(u64, u64)> {
    let total = data.len() as u64;
    let mut runs = Vec::new();
    let mut position = 0u64;
    while position < total {
        position += data[position as usize..]
            .iter()
            .take_while(|&&byte| byte == 0)
            .count() as u64;
        if position >= total {
            break;
        }
        let length = ((position + 1).next_multiple_of(BLOCK) - position).min(total - position);
        let mut seed = [0; SEED_SIZE];
        let found = Lfg::seed(
            &data[position as usize..(position + length) as usize],
            (position % BLOCK) as usize,
            &mut seed,
        ) as u64;
        if found > 0 {
            runs.push((position, position + found));
        }
        if let Some(file_system) = file_system
            && let Some((offset, size)) = file_system.file_at(position + found)
            && offset + size < position + length
        {
            position = offset + size;
            continue;
        }
        position += length;
    }
    runs
}

/// One row of results: sizes in bytes, RVZ's time in seconds.
#[derive(Debug)]
pub struct Row {
    pub level: i32,
    pub chunk_size: u32,
    pub rvz: u64,
    pub rvz_seconds: f64,
    pub plain: u64,
    pub dedup: u64,
    pub filler: u64,
    pub bcj: u64,
}

/// Write an RVZ of `input` to `scratch`, then model the levers on `disc`.
pub fn measure(
    input: &Path,
    scratch: &Path,
    disc: &Disc,
    layout: &Layout,
    level: i32,
    chunk_size: u32,
) -> Result<Row> {
    let options = rvz_rs::Options {
        compression: rvz_rs::Compression::Zstd,
        level,
        chunk_size,
        scrub: false,
    };
    let start = Instant::now();
    let stats = rvz_rs::compress(input, scratch, &options, &[], &mut |_| {})
        .with_context(|| format!("Failed to write \"{}\"", scratch.display()))?;
    let rvz_seconds = start.elapsed().as_secs_f64();
    std::fs::remove_file(scratch)?;
    let (plain, dedup, filler, bcj) = model(&disc.0, layout, level, chunk_size as u64)?;
    Ok(Row {
        level,
        chunk_size,
        rvz: stats.output_size,
        rvz_seconds,
        plain,
        dedup,
        filler,
        bcj,
    })
}

/// What one distinct chunk weighs under each lever.
struct Chunk {
    count: u64,
    plain: u64,
    filler: u64,
    bcj: u64,
}

/// The `plain`, `dedup`, `filler` and `bcj` sizes of a disc.
fn model(
    data: &[u8],
    layout: &Layout,
    level: i32,
    chunk_size: u64,
) -> Result<(u64, u64, u64, u64)> {
    let chunks = (data.len() as u64).div_ceil(chunk_size);
    // Identical chunks are measured once
    let mut distinct: HashMap<[u8; 20], (u64, u64)> = HashMap::new();
    for index in 0..chunks {
        let (start, end) = (
            index * chunk_size,
            ((index + 1) * chunk_size).min(data.len() as u64),
        );
        let digest: [u8; 20] = Sha1::digest(&data[start as usize..end as usize]).into();
        distinct.entry(digest).or_insert((index, 0)).1 += 1;
    }
    let measured: Vec<Chunk> = distinct
        .into_values()
        .collect::<Vec<_>>()
        .into_par_iter()
        .map(|(index, count)| {
            let start = index * chunk_size;
            let end = (start + chunk_size).min(data.len() as u64);
            measure_chunk(data, layout, level, start, end, count)
        })
        .collect::<Result<_>>()?;
    let index = chunks * INDEX_ENTRY;
    let sum = |size: fn(&Chunk) -> u64, every: bool| {
        index
            + measured
                .iter()
                .map(|chunk| size(chunk) * if every { chunk.count } else { 1 })
                .sum::<u64>()
    };
    Ok((
        sum(|chunk| chunk.plain, true),
        sum(|chunk| chunk.plain, false),
        sum(|chunk| chunk.filler, false),
        sum(|chunk| chunk.bcj, false),
    ))
}

fn measure_chunk(
    data: &[u8],
    layout: &Layout,
    level: i32,
    start: u64,
    end: u64,
    count: u64,
) -> Result<Chunk> {
    let original = &data[start as usize..end as usize];
    let plain = compressed_size(original, level)?;

    // Padding out, its seeds in, as RVZ stores a run longer than a seed
    let runs: Vec<(u64, u64)> = layout
        .filler
        .iter()
        .map(|&(run_start, run_end)| (run_start.max(start), run_end.min(end)))
        .filter(|(run_start, run_end)| {
            run_end > run_start && run_end - run_start > SEED_BYTES as u64
        })
        .collect();
    let seeds = runs.len() as u64 * SEED_BYTES as u64;
    let unfilled = |chunk: &[u8]| -> Vec<u8> {
        let mut kept = Vec::with_capacity(chunk.len());
        let mut at = start;
        for &(run_start, run_end) in &runs {
            kept.extend_from_slice(&chunk[(at - start) as usize..(run_start - start) as usize]);
            at = run_end;
        }
        kept.extend_from_slice(&chunk[(at - start) as usize..]);
        kept
    };
    let filler = if runs.is_empty() {
        plain
    } else {
        compressed_size(&unfilled(original), level)? + seeds
    };

    // The boot DOL's branches made absolute, positions counted from its start
    let bcj = match layout.dol {
        Some((dol_start, dol_end)) if dol_start < end && start < dol_end => {
            let (from, to) = (dol_start.max(start), dol_end.min(end));
            let mut filtered = original.to_vec();
            let mut writer = BcjWriter::new_ppc(Vec::new(), (from - dol_start) as usize);
            writer.write_all(&original[(from - start) as usize..(to - start) as usize])?;
            let converted = writer.finish()?;
            filtered[(from - start) as usize..(to - start) as usize].copy_from_slice(&converted);
            compressed_size(&unfilled(&filtered), level)? + if runs.is_empty() { 0 } else { seeds }
        }
        _ => filler,
    };

    Ok(Chunk {
        count,
        plain,
        filler,
        bcj,
    })
}

fn compressed_size(data: &[u8], level: i32) -> Result<u64> {
    // A chunk that does not compress is stored as it is, as RVZ stores it
    Ok((zstd::bulk::compress(data, level)?.len() as u64).min(data.len() as u64))
}

#[cfg(test)]
mod tests;

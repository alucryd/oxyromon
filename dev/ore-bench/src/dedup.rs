//! Dedup across discs (B6, GameCube arm): what each way of cutting a disc
//! into units finds again on other discs, against what smaller units cost in
//! compression and index.
//!
//! Each disc is taken as ORE would store it: its padding cut out, as its
//! seed is stored instead. What is left is cut by one of:
//!
//! - `fixed-N`: chunks of N bytes;
//! - `cdc-N`: FastCDC (2020) chunks of N bytes on average, from N/4 to 4N;
//! - `files`: each file of the file system whole, compressed in 2 MiB pieces,
//!   and what lies outside every file in 2 MiB chunks.
//!
//! [`units`] prints a row per unit and [`sum`] adds them up across discs, so
//! that a disc at a time is enough to hold.

use std::collections::HashSet;
use std::io::BufRead;

use anyhow::{Context, Result, bail};
use fastcdc::v2020::FastCDC;
use gcwii::disc::{PARTITION_DATA_SIZE, Platform, Source, Volume};
use gcwii::fst::FileSystem;
use gcwii::wii::{BLOCK_DATA_SIZE, BLOCK_TOTAL_SIZE};
use rayon::prelude::*;
use sha1::{Digest, Sha1};

use super::{Disc, find_filler};

const LEVEL: i32 = 19;
const PIECE: usize = 2 << 20;
/// What the index takes, in bytes: a reference a unit of a disc, and a
/// digest and location a unit stored.
pub const REFERENCE: u64 = 8;
pub const STORED: u64 = 28;

/// The schemes compared, by name.
pub const SCHEMES: &[&str] = &[
    "fixed-32K",
    "fixed-128K",
    "fixed-2M",
    "cdc-16K",
    "cdc-64K",
    "cdc-256K",
    "cdc-1M",
    "files",
];

/// One unit of a disc: its digest, length and compressed size.
pub struct Unit {
    pub digest: u64,
    pub length: u64,
    pub compressed: u64,
}

fn compressed_size(data: &[u8]) -> Result<u64> {
    data.chunks(PIECE)
        .map(|piece| Ok((zstd::bulk::compress(piece, LEVEL)?.len()).min(piece.len()) as u64))
        .sum()
}

fn unit(data: &[u8]) -> Result<Unit> {
    let digest = Sha1::digest(data);
    Ok(Unit {
        digest: u64::from_be_bytes(digest[..8].try_into()?),
        length: data.len() as u64,
        compressed: compressed_size(data)?,
    })
}

fn size(name: &str) -> usize {
    let (number, unit) = name.split_at(name.len() - 1);
    number.parse::<usize>().unwrap() << if unit == "M" { 20 } else { 10 }
}

/// Each scheme's units of an area.
pub type Schemes = Vec<(&'static str, Vec<Unit>)>;

/// Every scheme's units of a GameCube disc.
pub fn units(disc: &mut Disc, schemes: &[&'static str]) -> Result<Schemes> {
    let Some(mut volume) = Volume::new(disc, &[])? else {
        bail!("neither a GameCube nor a Wii disc");
    };
    if volume.platform != Platform::GameCube {
        bail!("a Wii disc: read it with wii_units");
    }
    let file_system = volume.file_system(disc, None)?;
    area_units(&disc.0, file_system.as_deref(), schemes)
}

/// Every scheme's units of a Wii disc, by part: each partition's data
/// decrypted and without its hashes, which RVZ recomputes rather than
/// stores, as `update`, `game` or `channel`, and the disc header and each
/// partition's ticket, TMD and certificates as `header`. What lies outside
/// every partition is left out, and its size returned.
pub fn wii_units(
    source: &mut dyn Source,
    schemes: &[&'static str],
) -> Result<(Vec<(String, Schemes)>, u64)> {
    let Some(mut volume) = Volume::new(source, &[])? else {
        bail!("neither a GameCube nor a Wii disc");
    };
    if volume.platform != Platform::Wii {
        bail!("a GameCube disc: read it with units");
    }
    let mut parts = Vec::new();
    let mut header = vec![0; volume.partitions.first().copied().unwrap_or(0) as usize];
    source.read_at(0, &mut header)?;
    let mut covered = header.len() as u64;
    for (index, &partition) in volume.partitions.clone().iter().enumerate() {
        let kind = match partition_type(source, partition)? {
            0 => "game",
            1 => "update",
            _ => "channel",
        };
        let data_offset = volume.data_offset(source, partition);
        let mut partition_header = vec![0; data_offset as usize];
        source.read_at(partition, &mut partition_header)?;
        header.extend_from_slice(&partition_header);
        let raw_size = volume.read_shifted(source, partition + PARTITION_DATA_SIZE, None)?;
        covered += data_offset + raw_size;
        let mut data = vec![0; (raw_size / BLOCK_TOTAL_SIZE * BLOCK_DATA_SIZE) as usize];
        volume.read(source, 0, &mut data, Some(partition))?;
        let file_system = volume.file_system(source, Some(partition))?;
        parts.push((
            format!("{kind}{index}"),
            area_units(&data, file_system.as_deref(), schemes)?,
        ));
    }
    parts.push(("header".into(), area_units(&header, None, schemes)?));
    Ok((parts, source.size().saturating_sub(covered)))
}

/// A partition's type, from the partition tables: 0 a game, 1 an update.
fn partition_type(source: &mut dyn Source, partition: u64) -> Result<u32> {
    let read_u32 = |source: &mut dyn Source, offset: u64| -> Result<u32> {
        let mut bytes = [0; 4];
        source.read_at(offset, &mut bytes)?;
        Ok(u32::from_be_bytes(bytes))
    };
    for group in 0..4 {
        let count = read_u32(source, 0x40000 + group * 8)?;
        let table = (read_u32(source, 0x40000 + group * 8 + 4)? as u64) << 2;
        for i in 0..count as u64 {
            if (read_u32(source, table + i * 8)? as u64) << 2 == partition {
                return read_u32(source, table + i * 8 + 4);
            }
        }
    }
    bail!("partition {partition:#x} is in no partition table")
}

/// Every scheme's units of an area laid out as a GameCube disc or a Wii
/// partition's data: its padding cut out first.
fn area_units(
    data: &[u8],
    file_system: Option<&FileSystem>,
    schemes: &[&'static str],
) -> Result<Schemes> {
    let filler = find_filler(data, file_system);

    // What is not padding, in order, and what of it is in no file
    let mut kept = Vec::with_capacity(data.len());
    let mut at = 0;
    for &(start, end) in &filler {
        kept.extend_from_slice(&data[at..start as usize]);
        at = end as usize;
    }
    kept.extend_from_slice(&data[at..]);
    let mut files: Vec<(u64, u64)> = file_system
        .map(|file_system| {
            file_system
                .files()
                .filter(|&(offset, size)| size > 0 && offset + size <= data.len() as u64)
                .collect()
        })
        .unwrap_or_default();
    files.sort_unstable();
    files.dedup();
    let mut outside = Vec::new();
    let mut at = 0u64;
    let cut = |from: u64, to: u64, out: &mut Vec<u8>| {
        // Between files, less the padding there
        let mut position = from;
        for &(start, end) in &filler {
            if end <= position || start >= to {
                continue;
            }
            out.extend_from_slice(&data[position as usize..start.max(position) as usize]);
            position = end.min(to);
        }
        if position < to {
            out.extend_from_slice(&data[position as usize..to as usize]);
        }
    };
    for &(offset, size) in &files {
        if offset > at {
            cut(at, offset, &mut outside);
        }
        at = at.max(offset + size);
    }
    cut(at.min(data.len() as u64), data.len() as u64, &mut outside);

    schemes
        .iter()
        .map(|&scheme| {
            let pieces: Vec<&[u8]> = if let Some(n) = scheme.strip_prefix("fixed-") {
                kept.chunks(size(n)).collect()
            } else if let Some(n) = scheme.strip_prefix("cdc-") {
                let average = size(n);
                FastCDC::new(&kept, average / 4, average, average * 4)
                    .map(|chunk| &kept[chunk.offset..chunk.offset + chunk.length])
                    .collect()
            } else {
                files
                    .iter()
                    .map(|&(offset, size)| &data[offset as usize..(offset + size) as usize])
                    .chain(outside.chunks(PIECE))
                    .collect()
            };
            let units = pieces.into_par_iter().map(unit).collect::<Result<_>>()?;
            Ok((scheme, units))
        })
        .collect()
}

/// A scheme's totals: bytes, then compressed with each disc alone and with
/// the whole set deduped, index included.
#[derive(Default)]
pub struct Total {
    pub length: u64,
    pub per_disc: u64,
    pub library: u64,
    pub references: u64,
    pub stored: u64,
}

/// Add up rows of `disc\tscheme\tdigest\tlength\tcompressed`, each disc's
/// rows together.
pub fn sum(rows: impl BufRead) -> Result<Vec<(&'static str, Total)>> {
    let mut totals: Vec<Total> = SCHEMES.iter().map(|_| Total::default()).collect();
    let mut library: HashSet<(usize, u64)> = HashSet::new();
    let mut disc_seen: HashSet<(usize, u64)> = HashSet::new();
    let mut current = String::new();
    for line in rows.lines().skip(1) {
        let line = line?;
        let fields: Vec<&str> = line.split('\t').collect();
        let [disc, scheme, digest, length, compressed] = fields[..] else {
            bail!("bad row: {line}");
        };
        if disc != current {
            current = disc.to_string();
            disc_seen.clear();
        }
        let scheme = SCHEMES
            .iter()
            .position(|&name| name == scheme)
            .with_context(|| format!("unknown scheme: {scheme}"))?;
        let digest = u64::from_str_radix(digest, 16).context("bad digest")?;
        let (length, compressed): (u64, u64) = (length.parse()?, compressed.parse()?);
        let total = &mut totals[scheme];
        total.length += length;
        total.references += 1;
        if disc_seen.insert((scheme, digest)) {
            total.per_disc += compressed + STORED;
        }
        if library.insert((scheme, digest)) {
            total.library += compressed + STORED;
            total.stored += 1;
        }
    }
    for total in &mut totals {
        total.per_disc += total.references * REFERENCE;
        total.library += total.references * REFERENCE;
    }
    Ok(SCHEMES.iter().copied().zip(totals).collect())
}

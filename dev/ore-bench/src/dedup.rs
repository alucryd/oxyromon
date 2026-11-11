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
use gcwii::disc::{Platform, Volume};
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

/// Every scheme's units of a GameCube disc.
pub fn units(disc: &mut Disc) -> Result<Vec<(&'static str, Vec<Unit>)>> {
    let Some(mut volume) = Volume::new(disc, &[])? else {
        bail!("neither a GameCube nor a Wii disc");
    };
    if volume.platform != Platform::GameCube {
        bail!("a Wii disc: dedup covers GameCube discs only");
    }
    let file_system = volume.file_system(disc, None)?;
    let data = &disc.0;
    let filler = find_filler(data, file_system.as_deref());

    // What is not padding, in disc order, and what of it is in no file
    let mut kept = Vec::with_capacity(data.len());
    let mut at = 0;
    for &(start, end) in &filler {
        kept.extend_from_slice(&data[at..start as usize]);
        at = end as usize;
    }
    kept.extend_from_slice(&data[at..]);
    let mut files: Vec<(u64, u64)> = file_system
        .as_deref()
        .map(|file_system| file_system.files().filter(|&(_, size)| size > 0).collect())
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
    cut(at, data.len() as u64, &mut outside);

    SCHEMES
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

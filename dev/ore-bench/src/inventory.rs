//! What a GameCube disc holds, file by file: the measure behind the
//! file-level levers (dedup by file, grouping by type, recompressing what is
//! compressed already) before any is built.
//!
//! Each file is compressed as RVZ's best setting in B5 compresses a disc:
//! Zstandard level 19, in independent 2 MiB pieces.

use anyhow::{Result, bail};
use gcwii::disc::{Platform, Source, Volume};
use rayon::prelude::*;
use sha1::{Digest, Sha1};

use super::Disc;

/// Zstandard's level and piece size, as RVZ's best setting in B5.
const LEVEL: i32 = 19;
const PIECE: usize = 2 << 20;

/// One file of a disc, or a part of its system area outside the file
/// system, named `<boot>`, `<dol>` or `<fst>`.
pub struct Entry {
    pub name: String,
    pub kind: &'static str,
    pub offset: u64,
    pub size: u64,
    pub compressed: u64,
    pub sha1: String,
}

/// What `data` is, from its first bytes: the formats GameCube games ship,
/// or `?` for none of them.
pub fn kind(data: &[u8]) -> &'static str {
    let magic = |bytes: &[u8]| data.starts_with(bytes);
    if magic(b"Yaz0") {
        "yaz0"
    } else if magic(b"Yay0") {
        "yay0"
    } else if magic(b"RARC") {
        "rarc"
    } else if magic(b"THP\0") {
        "thp"
    } else if magic(b"AFS\0") {
        "afs"
    } else if magic(b"@UTF") || magic(b"CPK ") {
        "cpk"
    } else if magic(b"STRM") {
        "ast"
    } else if magic(&[0x00, 0x00, 0x01, 0xBA]) {
        "mpeg-ps"
    } else if magic(b"BIK") {
        "bink"
    } else if magic(b"HVQM4") {
        "hvqm"
    } else if magic(b"RIFF") {
        "riff"
    } else if magic(b"BNR1") || magic(b"BNR2") {
        "banner"
    } else if data.len() >= 6 && data[0] == 0x80 && data[1] == 0x00 && {
        // An ADX header ends with "(c)CRI", just before where its data starts
        let start = u16::from_be_bytes([data[2], data[3]]) as usize + 4;
        start >= 6 && data.get(start - 6..start) == Some(b"(c)CRI")
    } {
        "adx"
    } else if data.iter().all(|&byte| byte == 0) {
        "zero"
    } else {
        "?"
    }
}

/// The size `data` compresses to, piece by piece, a piece that does not
/// compress counting as stored.
fn compressed_size(data: &[u8]) -> Result<u64> {
    data.par_chunks(PIECE)
        .map(|piece| {
            let size = zstd::bulk::compress(piece, LEVEL)?.len();
            Ok(size.min(piece.len()) as u64)
        })
        .sum()
}

/// Every file of a GameCube disc and its system area, in disc order.
pub fn inventory(disc: &mut Disc) -> Result<Vec<Entry>> {
    let Some(mut volume) = Volume::new(disc, &[])? else {
        bail!("neither a GameCube nor a Wii disc");
    };
    if volume.platform != Platform::GameCube {
        bail!("a Wii disc: the inventory covers GameCube discs only");
    }
    let mut regions: Vec<(String, u64, u64)> = Vec::new();
    // The disc header, its second half and the apploader, whose size its
    // header gives
    let mut header = [0; 8];
    disc.read_at(0x2454, &mut header)?;
    let apploader = 0x20
        + u32::from_be_bytes(header[..4].try_into()?) as u64
        + u32::from_be_bytes(header[4..].try_into()?) as u64;
    regions.push(("<boot>".into(), 0, 0x2440 + apploader));
    if let Some(dol) = volume.dol_offset(disc, None)? {
        let size = volume.dol_size(disc, None, dol)?;
        regions.push(("<dol>".into(), dol, size as u64));
    }
    let (fst, fst_size) = volume.fst(disc, None)?;
    regions.push(("<fst>".into(), fst, fst_size));
    if let Some(file_system) = volume.file_system(disc, None)? {
        regions.extend(file_system.named_files());
    }
    regions.sort_by_key(|&(_, offset, _)| offset);

    let data = &disc.0;
    regions
        .into_iter()
        .filter(|&(_, offset, size)| size > 0 && offset + size <= data.len() as u64)
        .map(|(name, offset, size)| {
            let bytes = &data[offset as usize..(offset + size) as usize];
            Ok(Entry {
                kind: kind(bytes),
                compressed: compressed_size(bytes)?,
                sha1: Sha1::digest(bytes)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
                name,
                offset,
                size,
            })
        })
        .collect()
}

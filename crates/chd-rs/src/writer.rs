//! Creating v5 CHDs: the writer side of the container.
//!
//! A new CHD is written in one pass: hunks of the input are read, hashed,
//! and compared against the hunks already seen — and, for a clone, against
//! the parent's — so only the hunks which are genuinely new are stored; the
//! others become self or parent references. The raw map is assembled while
//! the data is appended, then compressed and appended once every hunk is
//! known, and the header's offsets and hashes are patched in place at the
//! end. As everywhere in this crate the result is interchangeable with
//! `chdman`'s, not merely readable by this crate: every byte layout follows
//! `chd.cpp`.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use sha1::{Digest, Sha1};

use crate::bitstream::BitstreamOut;
use crate::cdrom;
use crate::codec::{self, CODEC_NONE};
use crate::container::{Chd, ChdInfo, MTAG_DVD, MTAG_HARD_DISK};
use crate::crc16::crc16;
use crate::error::{Error, Result};
use crate::huffman::HuffmanEncoder;

/// The suffix appended to the output's name while it is being written.
pub(crate) const PART_SUFFIX: &str = ".part";
/// The magic every CHD starts with.
const MAGIC: &[u8; 8] = b"MComprHD";
/// The size of a version 5 header, the only one this writer produces.
const HEADER_SIZE: usize = 124;
/// Where each patched header field lives.
const MAPOFFSET_OFFSET: usize = 40;
const METAOFFSET_OFFSET: usize = 48;
const RAWSHA1_OFFSET: usize = 64;
const SHA1_OFFSET: usize = 84;
const PARENT_SHA1_OFFSET: usize = 104;

/// How much the buffered writer accumulates before flushing to disk.
const PENDING_LIMIT: usize = 1 << 20;

/// The size in bytes of one entry of the compressed (v5) map.
const ENTRY_SIZE: usize = 12;

/// Map entry types beyond the codec slots: a slot is its own index in the
/// header's compression list, while these name how an entry refers to data
/// stored elsewhere instead of carrying any.
const TYPE_NONE: u8 = 4;
const TYPE_SELF: u8 = 5;
const TYPE_PARENT: u8 = 6;
/// The two run-length markers of the map's Huffman stream, and the five
/// promoted forms the encoder recognises in a raw entry. None of them ever
/// reaches the disk: the second pass of `compress_map` writes the promoted
/// forms as their base type, without the payload they dropped.
const TYPE_RLE_SMALL: u8 = 7;
const TYPE_RLE_LARGE: u8 = 8;
const TYPE_SELF_0: u8 = 9;
const TYPE_SELF_1: u8 = 10;
const TYPE_PARENT_SELF: u8 = 11;
const TYPE_PARENT_0: u8 = 12;
const TYPE_PARENT_1: u8 = 13;

/// The metadata flag which draws an entry into the overall SHA-1.
const METADATA_CHECKSUM: u8 = 0x01;

/// Creates a version 5 CHD from an input file.
///
/// `unit_bytes` is the size of the input's logical units — 512 for a hard
/// disk, 2048 for a DVD — and `hunk_bytes` the size of a hunk, a multiple of
/// it. `compression` holds up to four codec tags tried for each hunk, the
/// shortest result winning; a leading NONE slot stores the CHD uncompressed.
/// Passing a `parent` clones it: hunks found unchanged in the parent become
/// references, and the child stores only what the parent does not hold.
/// `metadata` entries are `(tag, flags, data)` triples written as given,
/// those flagged with the checksum bit drawn into the overall hash.
///
/// Like every entry point of this crate, progress is reported as the input
/// bytes consumed since the last call, adding up to the input's size. The
/// output lands on `<output>.part` and is renamed into place once complete.
#[allow(clippy::too_many_arguments)]
pub fn create(
    input: &Path,
    output: &Path,
    unit_bytes: u32,
    hunk_bytes: u32,
    compression: [u32; 4],
    parent: Option<&mut Chd>,
    metadata: &[(u32, u8, Vec<u8>)],
    progress: &mut dyn FnMut(u64),
) -> Result<()> {
    let size = std::fs::metadata(input)?.len();
    if size == 0 {
        return Err(Error::InvalidOption("input file is empty".to_string()));
    }
    let logical_size = logical_size(metadata, size)?;
    let mut source = FileSource {
        file: File::open(input)?,
        size,
    };
    write_part(
        &mut source,
        logical_size,
        output,
        unit_bytes,
        hunk_bytes,
        compression,
        parent,
        metadata,
        progress,
    )
}

/// Where a new CHD's logical bytes come from.
pub(crate) trait Source {
    /// Fills `buf` with the logical bytes at `offset`, returning how many
    /// input bytes that consumed, which is what progress reports. What it
    /// has no data for it may leave as it was, as chdman's readers do.
    fn read(&mut self, offset: u64, buf: &mut [u8]) -> Result<u64>;

    /// Metadata only known once every byte was read, which chdman writes
    /// after the map.
    fn late_metadata(&self) -> Vec<(u32, u8, Vec<u8>)> {
        Vec::new()
    }
}

/// A single input file, read in order. A hard disk geometry may describe
/// more than the file holds; what lies past its end is not read at all.
struct FileSource {
    file: File,
    size: u64,
}

impl Source for FileSource {
    fn read(&mut self, offset: u64, buf: &mut [u8]) -> Result<u64> {
        let take = buf
            .len()
            .min(usize::try_from(self.size.saturating_sub(offset)).unwrap_or(buf.len()));
        // past the end of the file, chdman reads nothing and leaves its
        // buffer as it was
        self.file.read_exact(&mut buf[..take])?;
        Ok(take as u64)
    }
}

/// Writes a CHD to `<output>.part`, renaming it into place once complete
/// and removing it when anything failed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn write_part(
    source: &mut dyn Source,
    logical_size: u64,
    output: &Path,
    unit_bytes: u32,
    hunk_bytes: u32,
    compression: [u32; 4],
    parent: Option<&mut Chd>,
    metadata: &[(u32, u8, Vec<u8>)],
    progress: &mut dyn FnMut(u64),
) -> Result<()> {
    let part = part_path(output);
    let result = create_inner(
        source,
        logical_size,
        &part,
        unit_bytes,
        hunk_bytes,
        compression,
        parent,
        metadata,
        progress,
    );
    match result {
        Ok(()) => {
            std::fs::rename(&part, output)?;
            Ok(())
        }
        Err(error) => {
            let _ = std::fs::remove_file(&part);
            Err(error)
        }
    }
}

pub(crate) fn part_path(output: &Path) -> PathBuf {
    let mut part = output.as_os_str().to_os_string();
    part.push(PART_SUFFIX);
    PathBuf::from(part)
}

/// Creates a version 5 hard disk CHD the way `chdman createhd` does.
///
/// `unit_bytes` is the sector size, restricted to the four sizes chdman
/// accepts. The image gains a `GDDD` metadata entry describing it as
/// cylinders, heads and sectors, and that geometry — not the file — sets the
/// CHD's logical size: an image smaller than its geometry describes gains
/// zero hunks at the end. `chs` sets the geometry outright; a clone inherits
/// the parent's, never a guess; anything else gets a guessed one.
#[allow(clippy::too_many_arguments)]
pub fn create_hd(
    input: &Path,
    output: &Path,
    unit_bytes: u32,
    hunk_bytes: u32,
    compression: [u32; 4],
    parent: Option<&mut Chd>,
    chs: Option<(u32, u32, u32)>,
    progress: &mut dyn FnMut(u64),
) -> Result<()> {
    if !matches!(unit_bytes, 512 | 1024 | 2048 | 4096) {
        return Err(Error::InvalidOption(format!(
            "sector size must be 512, 1024, 2048 or 4096, got {unit_bytes}"
        )));
    }
    let size = std::fs::metadata(input)?.len();
    let geometry = match chs {
        Some(geometry) => geometry,
        None => match parent.as_deref().and_then(|parent| {
            parent
                .metadata()
                .iter()
                .find(|entry| entry.tag == MTAG_HARD_DISK)
        }) {
            Some(entry) => {
                let Some([cylinders, heads, sectors, _]) = parse_geometry(&entry.data) else {
                    return Err(Error::InvalidOption(
                        "malformed hard disk metadata in the parent".to_string(),
                    ));
                };
                (cylinders, heads, sectors)
            }
            None => guess_chs(size, unit_bytes),
        },
    };
    let data = format!(
        "CYLS:{},HEADS:{},SECS:{},BPS:{unit_bytes}\0",
        geometry.0, geometry.1, geometry.2
    )
    .into_bytes();
    let metadata = [(MTAG_HARD_DISK, METADATA_CHECKSUM, data)];
    create(
        input,
        output,
        unit_bytes,
        hunk_bytes,
        compression,
        parent,
        &metadata,
        progress,
    )
}

/// Creates a version 5 DVD CHD the way `chdman createdvd` does: 2048-byte
/// sectors, chdman's default hunk being two of them, and an empty `DVD `
/// metadata entry marking the CHD as a DVD.
pub fn create_dvd(
    input: &Path,
    output: &Path,
    hunk_bytes: u32,
    compression: [u32; 4],
    parent: Option<&mut Chd>,
    progress: &mut dyn FnMut(u64),
) -> Result<()> {
    let size = std::fs::metadata(input)?.len();
    if !size.is_multiple_of(2048) {
        return Err(Error::InvalidOption(format!(
            "data size {size} is not divisible by sector size 2048"
        )));
    }
    // chdman writes the empty string, terminator included
    let metadata = [(MTAG_DVD, METADATA_CHECKSUM, vec![0u8])];
    create(
        input,
        output,
        2048,
        hunk_bytes,
        compression,
        parent,
        &metadata,
        progress,
    )
}

/// Creates a version 5 CD CHD the way `chdman createcd` does, from a CUE
/// sheet, a GDI or an ISO.
///
/// Units are 2448-byte frames, each track padded to a multiple of four;
/// `hunk_bytes` must be a multiple of 2448, chdman's default being eight
/// frames. Every track gets its `CHT2` entry, `CHGD` for a GD-ROM, and
/// sessions their `CHSE` entries. Progress adds up to
/// [`cdrom::input_size`](crate::cd_input_size).
pub fn create_cd(
    input: &Path,
    output: &Path,
    hunk_bytes: u32,
    compression: [u32; 4],
    parent: Option<&mut Chd>,
    progress: &mut dyn FnMut(u64),
) -> Result<()> {
    let (mut toc, info) = cdrom::parse_toc(input)?;
    // each track padded to a 4-frame boundary, which the reader undoes
    let mut totalsectors = 0u32;
    for track in &mut toc.tracks {
        let frames = track.frames as u32;
        let padded = frames.wrapping_add(cdrom::TRACK_PADDING - 1) / cdrom::TRACK_PADDING;
        let extraframes = padded
            .wrapping_mul(cdrom::TRACK_PADDING)
            .wrapping_sub(frames);
        track.extraframes = extraframes as i32;
        totalsectors = totalsectors.wrapping_add(frames.wrapping_add(extraframes));
    }
    let logical_size = u64::from(totalsectors) * u64::from(cdrom::FRAME_SIZE);
    let metadata = cdrom::metadata_entries(&toc);
    let input_size = cdrom::input_size(input)?;
    let mut source = cdrom::CdSource::new(&toc, &info, input_size, logical_size);
    write_part(
        &mut source,
        logical_size,
        output,
        cdrom::FRAME_SIZE,
        hunk_bytes,
        compression,
        parent,
        &metadata,
        progress,
    )
}

/// Guesses a hard disk geometry the way `chdman` does: the largest divisor
/// of the sector count below 64 becomes the sectors per track, the largest
/// divisor of the quotient below 17 the heads, and the sector count is
/// bumped until such a pair divides it exactly.
fn guess_chs(size: u64, unit_bytes: u32) -> (u32, u32, u32) {
    let mut total = size / u64::from(unit_bytes);
    loop {
        for sectors in (2..=63u32).rev() {
            if !total.is_multiple_of(u64::from(sectors)) {
                continue;
            }
            let total_heads = total / u64::from(sectors);
            for heads in (2..=16u32).rev() {
                if total_heads.is_multiple_of(u64::from(heads)) {
                    let cylinders =
                        u32::try_from(total_heads / u64::from(heads)).unwrap_or(u32::MAX);
                    return (cylinders, heads, sectors);
                }
            }
        }
        total = total.wrapping_add(1);
    }
}

/// Reads back a `GDDD` entry: the `CYLS:{c},HEADS:{h},SECS:{s},BPS:{bps}`
/// form of version 5, or the plain `{c},{h},{s},{bps}` of versions 3 and 4.
/// Text trailing the four numbers is ignored, the way chdman's `sscanf`
/// leaves it.
fn parse_geometry(data: &[u8]) -> Option<[u32; 4]> {
    let text = std::str::from_utf8(data).ok()?.split('\0').next()?;
    scan_geometry(text, ["", "", "", ""])
        .or_else(|| scan_geometry(text, ["CYLS:", "HEADS:", "SECS:", "BPS:"]))
}

fn scan_geometry(text: &str, prefixes: [&str; 4]) -> Option<[u32; 4]> {
    let mut rest = text;
    let mut values = [0u32; 4];
    for (index, (value, prefix)) in values.iter_mut().zip(prefixes).enumerate() {
        let body = rest.strip_prefix(prefix)?.trim_start();
        let end = body
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(body.len());
        if end == 0 {
            return None;
        }
        *value = body[..end].parse().ok()?;
        rest = &body[end..];
        if index < 3 {
            rest = rest.strip_prefix(',').unwrap_or(rest);
        }
    }
    Some(values)
}

#[allow(clippy::too_many_arguments)]
fn create_inner(
    source: &mut dyn Source,
    logical_size: u64,
    part: &Path,
    unit_bytes: u32,
    hunk_bytes: u32,
    compression: [u32; 4],
    parent: Option<&mut Chd>,
    metadata: &[(u32, u8, Vec<u8>)],
    progress: &mut dyn FnMut(u64),
) -> Result<()> {
    let parent_info = parent.as_ref().map(|parent| parent.info());
    let hunk_count = validate(
        logical_size,
        unit_bytes,
        hunk_bytes,
        &compression,
        parent_info.as_ref(),
        metadata,
    )?;
    let compressed = compression[0] != CODEC_NONE;
    let hunk_bytes64 = u64::from(hunk_bytes);

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(part)?;
    let mut header = [0u8; HEADER_SIZE];
    header[..8].copy_from_slice(MAGIC);
    header[8..12].copy_from_slice(&(HEADER_SIZE as u32).to_be_bytes());
    header[12..16].copy_from_slice(&5u32.to_be_bytes());
    for (index, tag) in compression.iter().enumerate() {
        header[16 + index * 4..20 + index * 4].copy_from_slice(&tag.to_be_bytes());
    }
    header[32..40].copy_from_slice(&logical_size.to_be_bytes());
    if !compressed {
        // an uncompressed CHD holds its map, four bytes a hunk, right after
        // the header, and carries no hash of its own
        header[MAPOFFSET_OFFSET..48].copy_from_slice(&(HEADER_SIZE as u64).to_be_bytes());
    }
    header[56..60].copy_from_slice(&hunk_bytes.to_be_bytes());
    header[60..64].copy_from_slice(&unit_bytes.to_be_bytes());
    if let Some(sha1) = parent_info.as_ref().and_then(|info| info.sha1.as_ref()) {
        header[PARENT_SHA1_OFFSET..HEADER_SIZE].copy_from_slice(sha1);
    }

    let mut out = FileWriter::new(&mut file);
    out.append(&header)?;

    let mut rawmap = Vec::new();
    let mut table = Vec::new();
    let mut table_pos = 0;
    if compressed {
        rawmap = vec![0u8; hunk_count as usize * ENTRY_SIZE];
    } else {
        table_pos = out.append_zeros(u64::from(hunk_count) * 4)?;
    }
    // like chdman, the metadata goes in before any hunk
    let last_metadata = write_metadata(&mut out, metadata, None)?;

    let mut ring = WorkRing::new(hunk_bytes, logical_size);
    let parent_map = match parent {
        Some(parent) => Some(ring.walk_parent(parent, unit_bytes, hunk_count, compressed)?),
        None => None,
    };

    let mut current_map: HashMap<(u16, [u8; 20]), u32> = HashMap::new();
    let mut rawsha1 = Sha1::new();
    let mut table_written: u64 = 0;
    for (done, numbytes) in ring.chunks() {
        let (data, hunks) = ring.fill(done, numbytes, |buf| source.read(done, buf), progress)?;
        // Each hunk's hash and best encoding are worked out on every core, the
        // running hash of the data alongside; which hunks refer to earlier
        // ones, and where the rest land, is then settled in order below, so
        // the CHD is the same as one written a hunk at a time. A hunk only a
        // later one in the same chunk repeats is compressed for nothing, as
        // in chdman.
        let precomputed: Vec<Precomputed> = if compressed {
            let (_, precomputed) = rayon::join(
                || rawsha1.update(&data[..numbytes]),
                || {
                    data.par_chunks_exact(hunk_bytes as usize)
                        .map(|hunk| {
                            let hash = crc_and_sha1(hunk);
                            let known = current_map.contains_key(&hash)
                                || parent_map
                                    .as_ref()
                                    .is_some_and(|map| map.contains_key(&hash));
                            let packed =
                                (!known).then(|| codec::find_best_compressor(&compression, hunk));
                            (hash, packed)
                        })
                        .collect()
                },
            );
            precomputed
        } else {
            Vec::new()
        };
        let mut precomputed = precomputed.into_iter();
        for index in 0..hunks {
            let hunknum = done / hunk_bytes64 + index as u64;
            let data = &data[index * hunk_bytes as usize..][..hunk_bytes as usize];
            if compressed {
                let (hash, packed) = precomputed.next().expect("a hunk's precomputed hash");
                if let Some(reference) = current_map.get(&hash) {
                    set_entry(&mut rawmap, hunknum, TYPE_SELF, 0, u64::from(*reference), 0);
                } else if let Some(unit) =
                    parent_map.as_ref().and_then(|map| map.get(&hash)).copied()
                {
                    set_entry(&mut rawmap, hunknum, TYPE_PARENT, 0, unit, 0);
                } else {
                    let (slot, packed) =
                        packed.unwrap_or_else(|| codec::find_best_compressor(&compression, data));
                    let (kind, stored) = if slot < 0 {
                        (TYPE_NONE, data)
                    } else {
                        (slot as u8, packed.as_slice())
                    };
                    let offset = out.append(stored)?;
                    set_entry(
                        &mut rawmap,
                        hunknum,
                        kind,
                        stored.len() as u32,
                        offset,
                        hash.0,
                    );
                    current_map.insert(hash, hunknum as u32);
                }
            } else {
                if data.iter().all(|byte| *byte == 0) {
                    table.extend_from_slice(&0u32.to_be_bytes());
                } else {
                    let offset = out.append_aligned(data, hunk_bytes64)?;
                    table.extend_from_slice(&((offset / hunk_bytes64) as u32).to_be_bytes());
                }
                if table.len() >= PENDING_LIMIT {
                    out.write_at(table_pos + table_written, &table)?;
                    table_written += table.len() as u64;
                    table.clear();
                }
            }
        }
    }

    if compressed {
        let raw: [u8; 20] = rawsha1.finalize().into();
        let map = compress_map(&rawmap, hunk_count, unit_bytes, hunk_bytes)?;
        let map_offset = out.append(&map)?;
        out.write_at(MAPOFFSET_OFFSET as u64, &map_offset.to_be_bytes())?;
        out.write_at(RAWSHA1_OFFSET as u64, &raw)?;
        write_metadata(&mut out, &source.late_metadata(), last_metadata)?;
        let overall = overall_sha1(&raw, metadata);
        out.write_at(SHA1_OFFSET as u64, &overall)?;
    } else {
        if !table.is_empty() {
            out.write_at(table_pos + table_written, &table)?;
        }
    }
    out.flush()?;
    Ok(())
}

/// Checks the settings against the logical size and the parent, returning
/// the CHD's hunk count.
fn validate(
    logical_size: u64,
    unit_bytes: u32,
    hunk_bytes: u32,
    compression: &[u32; 4],
    parent: Option<&ChdInfo>,
    metadata: &[(u32, u8, Vec<u8>)],
) -> Result<u32> {
    if unit_bytes == 0 {
        return Err(Error::InvalidOption("unit size cannot be 0".to_string()));
    }
    if hunk_bytes == 0 {
        return Err(Error::InvalidOption("hunk size cannot be 0".to_string()));
    }
    if hunk_bytes < unit_bytes || !hunk_bytes.is_multiple_of(unit_bytes) {
        return Err(Error::InvalidOption(format!(
            "hunk size {hunk_bytes} is not a multiple of unit size {unit_bytes}"
        )));
    }
    let mut uncompressed = false;
    for tag in compression {
        if *tag == CODEC_NONE {
            uncompressed = true;
        } else {
            if uncompressed {
                return Err(Error::InvalidOption(
                    "a compression slot follows an uncompressed one".to_string(),
                ));
            }
            if !codec::is_known(*tag) {
                return Err(Error::InvalidOption(format!(
                    "unknown compression {}",
                    crate::fourcc(*tag)
                )));
            }
        }
    }
    for (tag, _flags, data) in metadata {
        if data.is_empty() {
            return Err(Error::InvalidOption(format!(
                "metadata {} cannot be empty",
                crate::fourcc(*tag)
            )));
        }
        if data.len() >= 1 << 24 {
            return Err(Error::InvalidOption(format!(
                "metadata {} is too large",
                crate::fourcc(*tag)
            )));
        }
    }
    if let Some(parent) = parent {
        if parent.version < 3 {
            return Err(Error::Unsupported(format!(
                "cannot clone a version {} CHD",
                parent.version
            )));
        }
        if parent.unit_size != unit_bytes {
            return Err(Error::InvalidOption(format!(
                "a cloned CHD must use its parent's unit size {}",
                parent.unit_size
            )));
        }
    }
    if logical_size == 0 {
        return Err(Error::InvalidOption("input is empty".to_string()));
    }
    if logical_size > u64::from(u32::MAX) * u64::from(hunk_bytes) {
        return Err(Error::InvalidOption(
            "input file is too large for one CHD".to_string(),
        ));
    }
    Ok(logical_size.div_ceil(u64::from(hunk_bytes)) as u32)
}

/// The size the CHD covers: a hard disk geometry sets it, and without a
/// `GDDD` entry it is simply the input's own size. A file smaller than its
/// geometry describes gains zero hunks at the end; one larger keeps only
/// what the geometry covers.
fn logical_size(metadata: &[(u32, u8, Vec<u8>)], size: u64) -> Result<u64> {
    let Some(entry) = metadata.iter().find(|entry| entry.0 == MTAG_HARD_DISK) else {
        return Ok(size);
    };
    let Some([cylinders, heads, sectors, bps]) = parse_geometry(&entry.2) else {
        return Err(Error::InvalidOption(
            "malformed hard disk metadata".to_string(),
        ));
    };
    let Some(product) = u64::from(cylinders)
        .checked_mul(u64::from(heads))
        .and_then(|value| value.checked_mul(u64::from(sectors)))
        .and_then(|value| value.checked_mul(u64::from(bps)))
        .filter(|product| *product > 0)
    else {
        return Err(Error::InvalidOption(
            "invalid hard disk geometry".to_string(),
        ));
    };
    if !size.is_multiple_of(u64::from(bps)) {
        return Err(Error::InvalidOption(format!(
            "data size {size} is not divisible by sector size {bps}"
        )));
    }
    Ok(product)
}

/// The hunks chdman's compressor keeps in flight: its work buffer holds
/// 256 hunks, plus one, and is filled half at a time.
const WORK_BUFFER_HUNKS: u64 = 256;

/// chdman's work buffer, which is never cleared. The bytes of the last hunk
/// past the logical end, and past the end of a raw input, are whatever the
/// buffer last held there: zeros, the data of 256 hunks before, or the
/// parent's when cloning. They are hashed and compressed along with the
/// rest, so a byte-identical CHD has to reproduce them.
struct WorkRing {
    buffer: Vec<u8>,
    hunk_bytes: u64,
    logical_size: u64,
}

impl WorkRing {
    fn new(hunk_bytes: u32, logical_size: u64) -> Self {
        let hunk_bytes = u64::from(hunk_bytes);
        Self {
            buffer: vec![0u8; ((WORK_BUFFER_HUNKS + 1) * hunk_bytes) as usize],
            hunk_bytes,
            logical_size,
        }
    }

    /// The reads chdman makes: half a buffer at a time, the last one short.
    fn chunks(&self) -> Vec<(u64, usize)> {
        let half = WORK_BUFFER_HUNKS / 2 * self.hunk_bytes;
        let mut chunks = Vec::new();
        let mut done = 0;
        while done < self.logical_size {
            let numbytes = half.min(self.logical_size - done);
            chunks.push((done, numbytes as usize));
            done += numbytes;
        }
        chunks
    }

    /// Where in the buffer the chunk at `done` lands.
    fn position(&self, done: u64) -> usize {
        (done % (WORK_BUFFER_HUNKS * self.hunk_bytes)) as usize
    }

    /// Reads a chunk into place with `read`, which may leave part of what it
    /// is given untouched, and returns the chunk's whole hunks.
    fn fill(
        &mut self,
        done: u64,
        numbytes: usize,
        read: impl FnOnce(&mut [u8]) -> Result<u64>,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(&[u8], usize)> {
        let position = self.position(done);
        progress(read(&mut self.buffer[position..position + numbytes])?);
        let hunks = (numbytes as u64).div_ceil(self.hunk_bytes) as usize;
        let length = hunks * self.hunk_bytes as usize;
        Ok((&self.buffer[position..position + length], hunks))
    }

    /// Walks the parent the way chdman does before compressing: its hunks
    /// read into the buffer, one more than each chunk holds so the last
    /// windows are whole, a hunk the parent does not have leaving the buffer
    /// as it was. Every hunk-sized window, at each of the child's unit
    /// offsets, maps to the first parent unit it was seen at, the value a
    /// parent reference carries; the last hunk, and every hunk of an
    /// uncompressed CHD, only at its first unit.
    fn walk_parent(
        &mut self,
        parent: &mut Chd,
        unit_bytes: u32,
        hunk_count: u32,
        compressed: bool,
    ) -> Result<HashMap<(u16, [u8; 20]), u64>> {
        let hunk_bytes = self.hunk_bytes as usize;
        let parent_hunks = parent.info().hunk_count;
        let uph = self.hunk_bytes / u64::from(unit_bytes);
        let mut map: HashMap<(u16, [u8; 20]), u64> = HashMap::new();
        for (done, numbytes) in self.chunks() {
            let position = self.position(done);
            let end = done + numbytes as u64;
            let mut curoffs = done;
            let mut slot = position;
            let mut curhunk = done / self.hunk_bytes;
            while curoffs < end + 1 {
                if curhunk < parent_hunks {
                    parent.read_hunk(curhunk as u32, &mut self.buffer[slot..slot + hunk_bytes])?;
                }
                curoffs += self.hunk_bytes;
                slot += hunk_bytes;
                curhunk += 1;
            }
            if !compressed {
                // the map is only for parent references, which an
                // uncompressed CHD does not make
                continue;
            }
            let mut curoffs = done;
            while curoffs < end {
                let hunknum = curoffs / self.hunk_bytes;
                let units = if hunknum == u64::from(hunk_count) - 1 {
                    1
                } else {
                    uph
                };
                let base = position + (curoffs - done) as usize;
                for unit in 0..units {
                    let start = base + (unit * u64::from(unit_bytes)) as usize;
                    map.entry(crc_and_sha1(&self.buffer[start..start + hunk_bytes]))
                        .or_insert(hunknum * uph + unit);
                }
                curoffs += self.hunk_bytes;
            }
        }
        Ok(map)
    }
}

/// A hunk's identity, and its best encoding unless an earlier hunk had the
/// same identity: the slot that won, or -1 for none, and its bytes.
type Precomputed = ((u16, [u8; 20]), Option<(i8, Vec<u8>)>);

/// The identity a hunk is recognised by: its CRC-32-IEEE stored as the
/// map's 16-bit checksum, next to its SHA-1.
fn crc_and_sha1(data: &[u8]) -> (u16, [u8; 20]) {
    (crc16(data), Sha1::digest(data).into())
}

/// Records one entry of the raw compressed map: the type, its 24-bit
/// compressed length, its 48-bit offset or reference, and the 16-bit
/// checksum of the hunk itself.
fn set_entry(map: &mut [u8], hunknum: u64, kind: u8, complen: u32, offset: u64, crc: u16) {
    if map.is_empty() {
        return;
    }
    let base = hunknum as usize * ENTRY_SIZE;
    map[base] = kind;
    map[base + 1..base + 4].copy_from_slice(&complen.to_be_bytes()[1..]);
    map[base + 4..base + 10].copy_from_slice(&offset.to_be_bytes()[2..]);
    map[base + 10..base + 12].copy_from_slice(&crc.to_be_bytes());
}

fn entry_type(entry: &[u8]) -> u8 {
    entry[0]
}

fn entry_complen(entry: &[u8]) -> u32 {
    u32::from_be_bytes([0, entry[1], entry[2], entry[3]])
}

fn entry_offset(entry: &[u8]) -> u64 {
    u64::from_be_bytes([
        0, 0, entry[4], entry[5], entry[6], entry[7], entry[8], entry[9],
    ])
}

fn entry_crc(entry: &[u8]) -> u16 {
    u16::from_be_bytes([entry[10], entry[11]])
}

/// The number of bits a value needs, the width MAME's `bit_width` gives the
/// map's fields.
fn bit_width(value: u64) -> u32 {
    u64::BITS - value.leading_zeros()
}

/// Writes a value which may span more than the stream's 32-bit window, the
/// high half first.
fn write_bits(bits: &mut BitstreamOut<'_>, value: u64, count: u32) {
    if count == 0 {
        return;
    }
    if count > 32 {
        bits.write((value >> 32) as u32, count - 32);
        bits.write(value as u32, 32);
    } else {
        bits.write(value as u32, count);
    }
}

/// Compresses the raw map the way `chd.cpp`'s `compress_v5_map` does, in
/// five passes: the CRC of the raw map; a run-length pass which feeds a
/// 16-symbol Huffman histogram and emits the types as a symbol stream,
/// promoting repeated SELF and PARENT entries to marker-free forms and
/// collapsing runs of three or more into small or large RLE codes; the
/// table export and encoding of that stream; a second walk which, reading
/// the symbol stream back, writes each entry's surviving payload bits; and
/// the map's 16-byte header.
fn compress_map(
    rawmap: &[u8],
    hunk_count: u32,
    unit_bytes: u32,
    hunk_bytes: u32,
) -> Result<Vec<u8>> {
    let uph = u64::from(hunk_bytes / unit_bytes);
    let hunkcount = u64::from(hunk_count);
    let mapcrc = crc16(rawmap);

    // pass 1: build the symbol stream, promoting entries whose payload the
    // second pass will be able to omit
    let mut encoder = HuffmanEncoder::new(16, 8);
    let mut rle: Vec<u8> = Vec::new();
    let mut last_self: u32 = 0;
    let mut max_self: u32 = 0;
    let mut last_parent: u64 = 0;
    let mut max_parent: u64 = 0;
    let mut max_complen: u32 = 0;
    let mut lastcomp: u8 = 0;
    let mut count: i64 = 0;
    let mut hunknum = 0u64;
    while hunknum < hunkcount {
        let entry = &rawmap[hunknum as usize * ENTRY_SIZE..][..ENTRY_SIZE];
        let mut curcomp = entry_type(entry);
        match curcomp {
            TYPE_SELF => {
                let refhunk = entry_offset(entry) as u32;
                if refhunk == last_self {
                    curcomp = TYPE_SELF_0;
                } else if refhunk == last_self + 1 {
                    curcomp = TYPE_SELF_1;
                } else {
                    max_self = max_self.max(refhunk);
                }
                last_self = refhunk;
            }
            TYPE_PARENT => {
                let refunit = entry_offset(entry);
                if refunit == hunknum * uph {
                    curcomp = TYPE_PARENT_SELF;
                } else if refunit == last_parent {
                    curcomp = TYPE_PARENT_0;
                } else if refunit == last_parent + uph {
                    curcomp = TYPE_PARENT_1;
                } else {
                    max_parent = max_parent.max(refunit);
                }
                last_parent = refunit;
            }
            _ => max_complen = max_complen.max(entry_complen(entry)),
        }
        if curcomp == lastcomp {
            count += 1;
        }
        if curcomp != lastcomp || hunknum == hunkcount - 1 {
            while count != 0 {
                if count < 3 {
                    rle.push(lastcomp);
                    encoder.histo_one(usize::from(lastcomp));
                    count -= 1;
                } else if count <= 18 {
                    rle.push(TYPE_RLE_SMALL);
                    encoder.histo_one(usize::from(TYPE_RLE_SMALL));
                    rle.push((count - 3) as u8);
                    encoder.histo_one((count - 3) as usize);
                    count = 0;
                } else {
                    let this = count.min(274);
                    rle.push(TYPE_RLE_LARGE);
                    encoder.histo_one(usize::from(TYPE_RLE_LARGE));
                    rle.push(((this - 19) >> 4) as u8);
                    encoder.histo_one(usize::from(((this - 19) >> 4) as u8));
                    rle.push(((this - 19) & 15) as u8);
                    encoder.histo_one(usize::from(((this - 19) & 15) as u8));
                    count -= this;
                }
            }
            if curcomp != lastcomp {
                rle.push(curcomp);
                encoder.histo_one(usize::from(curcomp));
                lastcomp = curcomp;
            }
        }
        hunknum += 1;
    }

    // pass 2 setup: the widths every entry's payload must fit in, then the
    // bit budget the whole stream needs
    let lengthbits = bit_width(u64::from(max_complen));
    let selfbits = bit_width(u64::from(max_self));
    let parentbits = bit_width(max_parent);
    let widest = u64::from(lengthbits + 16)
        .max(u64::from(selfbits))
        .max(u64::from(parentbits));
    let nbits = 8 * 16 + (12 + widest) * hunkcount;
    let mut compressed = vec![0u8; (nbits / 8 + 1) as usize];

    {
        let mut bits = BitstreamOut::new(&mut compressed[16..]);
        encoder.compute_tree_from_histo()?;
        encoder.export_tree_rle(&mut bits)?;
        for symbol in &rle {
            encoder.encode_one(&mut bits, usize::from(*symbol));
        }

        // pass 3: walk the raw map again, replaying the symbol stream to
        // know in hand which form each entry took, and write only the
        // payload bits the decoder will not reconstruct on its own
        let mut lastcomp: u8 = 0;
        let mut count: i64 = 0;
        let mut cursor = 0usize;
        let mut firstoffs = 0u64;
        let mut hunknum = 0u64;
        while hunknum < hunkcount {
            let entry = &rawmap[hunknum as usize * ENTRY_SIZE..][..ENTRY_SIZE];
            if count == 0 {
                let val = rle[cursor];
                cursor += 1;
                match val {
                    TYPE_RLE_SMALL => {
                        count = 2 + i64::from(rle[cursor]);
                        cursor += 1;
                    }
                    TYPE_RLE_LARGE => {
                        count = 18 + (i64::from(rle[cursor]) << 4) + i64::from(rle[cursor + 1]);
                        cursor += 2;
                    }
                    _ => lastcomp = val,
                }
            } else {
                count -= 1;
            }
            let length = entry_complen(entry);
            let offset = entry_offset(entry);
            let crc = entry_crc(entry);
            match lastcomp {
                TYPE_SELF => write_bits(&mut bits, offset, selfbits),
                TYPE_PARENT => write_bits(&mut bits, offset, parentbits),
                TYPE_NONE => {
                    write_bits(&mut bits, u64::from(crc), 16);
                    if firstoffs == 0 {
                        firstoffs = offset;
                    }
                }
                TYPE_RLE_SMALL | TYPE_RLE_LARGE | TYPE_SELF_0 | TYPE_SELF_1 | TYPE_PARENT_SELF
                | TYPE_PARENT_0 | TYPE_PARENT_1 => {}
                _ => {
                    write_bits(&mut bits, u64::from(length), lengthbits);
                    write_bits(&mut bits, u64::from(crc), 16);
                    if firstoffs == 0 {
                        firstoffs = offset;
                    }
                }
            }
            hunknum += 1;
        }

        // the map's own header closes the stream out
        let complen = bits.flush();
        if bits.overflow() {
            return Err(Error::Compression("map does not fit the CHD".to_string()));
        }
        let complen = u32::try_from(complen)
            .map_err(|_| Error::Compression("map does not fit the CHD".to_string()))?;
        compressed[..4].copy_from_slice(&complen.to_be_bytes());
        compressed[4..10].copy_from_slice(&firstoffs.to_be_bytes()[2..]);
        compressed[10..12].copy_from_slice(&mapcrc.to_be_bytes());
        compressed[12] = lengthbits as u8;
        compressed[13] = selfbits as u8;
        compressed[14] = parentbits as u8;
        compressed[15] = 0;
        compressed.truncate(complen as usize + 16);
    }
    Ok(compressed)
}

/// Appends the metadata list — each entry a 16-byte header followed by its
/// data, chained to the next by offset — after the entry at `previous`, or
/// as the list's start in the header. Returns the last entry's offset.
fn write_metadata(
    out: &mut FileWriter<'_>,
    entries: &[(u32, u8, Vec<u8>)],
    mut previous: Option<u64>,
) -> Result<Option<u64>> {
    for (tag, flags, data) in entries {
        let mut header = [0u8; 16];
        header[..4].copy_from_slice(&tag.to_be_bytes());
        header[4] = *flags;
        let len = u32::try_from(data.len())
            .map_err(|_| Error::InvalidOption("metadata entry is too large".to_string()))?;
        header[5..8].copy_from_slice(&len.to_be_bytes()[1..]);
        let start = out.append(&header)?;
        out.append(data)?;
        match previous {
            Some(previous) => out.write_at(previous + 8, &start.to_be_bytes())?,
            None => out.write_at(METAOFFSET_OFFSET as u64, &start.to_be_bytes())?,
        }
        previous = Some(start);
    }
    Ok(previous)
}

/// The header's overall hash: the raw hash, then the hashes of the
/// checksummed metadata entries in sorted order — exactly what the reader
/// recomputes to verify a CHD it cannot re-derive.
fn overall_sha1(raw: &[u8; 20], metadata: &[(u32, u8, Vec<u8>)]) -> [u8; 20] {
    let mut hasher = Sha1::new();
    hasher.update(raw);
    let mut hashes: Vec<Vec<u8>> = metadata
        .iter()
        .filter(|(_tag, flags, _data)| flags & METADATA_CHECKSUM != 0)
        .map(|(tag, _flags, data)| {
            let mut entry = tag.to_be_bytes().to_vec();
            entry.extend_from_slice(&Sha1::digest(data));
            entry
        })
        .collect();
    hashes.sort();
    for entry in &hashes {
        hasher.update(entry);
    }
    hasher.finalize().into()
}

/// A file which only ever grows at the end — the map's offsets are the
/// positions data lands at as it is appended — buffering the tail so
/// sequential appends reach the disk in bulk. Random writes into earlier
/// positions flush the buffer first, keeping the file consistent with the
/// writer's notion of where its end is.
struct FileWriter<'a> {
    file: &'a mut File,
    end: u64,
    pending: Vec<u8>,
}

impl<'a> FileWriter<'a> {
    fn new(file: &'a mut File) -> Self {
        Self {
            file,
            end: 0,
            pending: Vec::new(),
        }
    }

    /// Appends data at the current end, returning the offset it starts at.
    fn append(&mut self, data: &[u8]) -> Result<u64> {
        if self.pending.len() + data.len() > PENDING_LIMIT {
            self.flush()?;
        }
        let start = self.end;
        self.pending.extend_from_slice(data);
        self.end += data.len() as u64;
        Ok(start)
    }

    /// Appends zeroes at the current end without holding them in memory,
    /// returning the offset they start at.
    fn append_zeros(&mut self, count: u64) -> Result<u64> {
        self.flush()?;
        let start = self.end;
        // a write_at may have left the file anywhere before its end
        self.file.seek(SeekFrom::Start(start))?;
        let mut left = count;
        while left > 0 {
            let chunk = usize::try_from(left.min(1 << 16)).unwrap_or(1 << 16);
            self.file.write_all(&vec![0u8; chunk])?;
            left -= chunk as u64;
        }
        self.end += count;
        Ok(start)
    }

    /// Appends data aligned to `align`, padding the gap before it with
    /// zeroes, returning the offset the data itself starts at.
    fn append_aligned(&mut self, data: &[u8], align: u64) -> Result<u64> {
        self.append_zeros((align - self.end % align) % align)?;
        self.append(data)
    }

    /// Writes over bytes already committed before the end.
    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        self.flush()?;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(data)?;
        Ok(())
    }

    /// Pushes the buffered tail to disk. The position the file ends at
    /// afterwards is exactly where the writer believes it ends.
    fn flush(&mut self) -> Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let start = self.end - self.pending.len() as u64;
        self.file.seek(SeekFrom::Start(start))?;
        self.file.write_all(&self.pending)?;
        self.pending.clear();
        self.file.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{CODEC_LZMA, CODEC_ZLIB, CODEC_ZSTD};
    use crate::container::VerifyOutcome;
    use tempfile::TempDir;

    fn tag(name: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*name)
    }

    fn random_bytes(mut state: u64, len: usize) -> Vec<u8> {
        let mut data = Vec::with_capacity(len);
        while data.len() < len {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            data.extend_from_slice(&state.to_le_bytes());
        }
        data.truncate(len);
        data
    }

    fn write(
        input: &Path,
        output: &Path,
        unit_bytes: u32,
        hunk_bytes: u32,
        compression: [u32; 4],
        parent: Option<&mut Chd>,
        metadata: &[(u32, u8, Vec<u8>)],
    ) -> Vec<u64> {
        let mut seen = Vec::new();
        create(
            input,
            output,
            unit_bytes,
            hunk_bytes,
            compression,
            parent,
            metadata,
            &mut |delta| seen.push(delta),
        )
        .unwrap();
        seen
    }

    #[test]
    fn compressed_roundtrip() {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("input.bin");
        let output = dir.path().join("output.chd");
        let data = random_bytes(0x2545_F491_4F6C_DD1D, 700_000);
        std::fs::write(&input, &data).unwrap();
        write(
            &input,
            &output,
            512,
            4096,
            [CODEC_ZSTD, CODEC_ZLIB, CODEC_NONE, CODEC_NONE],
            None,
            &[],
        );

        let mut chd = Chd::open(&output).unwrap();
        let info = chd.info();
        assert_eq!(info.logical_size, data.len() as u64);
        assert_eq!(info.hunk_size, 4096);
        assert_eq!(info.unit_size, 512);
        assert_eq!(info.version, 5);
        let mut read_back = vec![0u8; data.len()];
        chd.read_bytes(0, &mut read_back).unwrap();
        assert_eq!(read_back, data);
        assert!(matches!(chd.verify().unwrap(), VerifyOutcome::Ok));
    }

    #[test]
    fn repeated_hunks_refer_to_themselves() {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("input.bin");
        let output = dir.path().join("output.chd");
        let block = random_bytes(0x9E37_79B9_7F4A_7C15, 65_536);
        let mut data = Vec::new();
        for _ in 0..32 {
            data.extend_from_slice(&block);
        }
        std::fs::write(&input, &data).unwrap();
        write(
            &input,
            &output,
            512,
            4096,
            [CODEC_ZLIB, CODEC_NONE, CODEC_NONE, CODEC_NONE],
            None,
            &[],
        );

        // only the first occurrence is stored, so the CHD is a fraction of
        // even one compressed copy of the block
        let size = std::fs::metadata(&output).unwrap().len();
        assert!(
            size < data.len() as u64 / 4,
            "stored {size} of {}",
            data.len()
        );
        let mut chd = Chd::open(&output).unwrap();
        let mut read_back = vec![0u8; data.len()];
        chd.read_bytes(0, &mut read_back).unwrap();
        assert_eq!(read_back, data);
        assert!(matches!(chd.verify().unwrap(), VerifyOutcome::Ok));
    }

    #[test]
    fn uncompressed_roundtrip() {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("input.bin");
        let output = dir.path().join("output.chd");
        let mut data = random_bytes(0xBF58_476D_1CE4_E5B9, 100_352);
        // a fully zero hunk is not stored, only pointed at as one
        data[4096..8192].fill(0);
        std::fs::write(&input, &data).unwrap();
        write(
            &input,
            &output,
            512,
            4096,
            [CODEC_NONE, CODEC_NONE, CODEC_NONE, CODEC_NONE],
            None,
            &[],
        );

        let mut chd = Chd::open(&output).unwrap();
        assert_eq!(chd.info().compression[0], CODEC_NONE);
        let mut read_back = vec![0u8; data.len()];
        chd.read_bytes(0, &mut read_back).unwrap();
        assert_eq!(read_back, data);
        assert!(matches!(chd.verify().unwrap(), VerifyOutcome::Uncompressed));
    }

    #[test]
    fn cloned_hunks_refer_to_the_parent() {
        let dir = TempDir::new().unwrap();
        let parent_input = dir.path().join("parent.bin");
        let parent_chd = dir.path().join("parent.chd");
        let child_input = dir.path().join("child.bin");
        let child_chd = dir.path().join("child.chd");

        let parent_data = random_bytes(0x94D0_49BB_1331_11EB, 64 * 1024);
        std::fs::write(&parent_input, &parent_data).unwrap();
        create_hd(
            &parent_input,
            &parent_chd,
            512,
            4096,
            [CODEC_LZMA, CODEC_NONE, CODEC_NONE, CODEC_NONE],
            None,
            None,
            &mut |_| {},
        )
        .unwrap();

        // the child starts with the parent's first four hunks, which should
        // become parent references, followed by data of its own
        let mut child_data = parent_data[..4 * 4096].to_vec();
        child_data.extend_from_slice(&random_bytes(0x7A5B_22E1_0D3E_F9C1, 8 * 1024));
        std::fs::write(&child_input, &child_data).unwrap();
        let mut parent = Chd::open(&parent_chd).unwrap();
        let parent_sha1 = parent.info().sha1;
        // the child is smaller than the parent but inherits its geometry, so
        // it gains zero hunks past its own data to cover the same size
        create_hd(
            &child_input,
            &child_chd,
            512,
            4096,
            [CODEC_LZMA, CODEC_NONE, CODEC_NONE, CODEC_NONE],
            Some(&mut parent),
            None,
            &mut |_| {},
        )
        .unwrap();

        let mut child = Chd::open_with_parent(&child_chd, &parent_chd).unwrap();
        assert_eq!(child.info().parent_sha1, parent_sha1);
        let mut read_back = vec![0u8; child_data.len()];
        child.read_bytes(0, &mut read_back).unwrap();
        assert_eq!(read_back, child_data);
        assert!(matches!(child.verify().unwrap(), VerifyOutcome::Ok));
    }

    #[test]
    fn metadata_survives_the_roundtrip() {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("input.bin");
        let output = dir.path().join("output.chd");
        let data = random_bytes(0x5DE6_CE66_24A9_408F, 20_480);
        std::fs::write(&input, &data).unwrap();
        let geometry = b"CYLS:2,HEADS:4,SECS:5,BPS:512".to_vec();
        write(
            &input,
            &output,
            512,
            4096,
            [CODEC_ZLIB, CODEC_NONE, CODEC_NONE, CODEC_NONE],
            None,
            &[
                (tag(b"GDDD"), METADATA_CHECKSUM, geometry.clone()),
                (tag(b"MIDE"), 0, b"disc.bin".to_vec()),
            ],
        );

        let mut chd = Chd::open(&output).unwrap();
        // the geometry covers exactly the input, and sets the logical size
        assert_eq!(chd.info().logical_size, 20_480);
        let entries = chd.metadata();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].tag, tag(b"GDDD"));
        assert_eq!(entries[0].data, geometry);
        assert_eq!(entries[1].tag, tag(b"MIDE"));
        // the header's overall hash covers the checksummed entry and the
        // reader recomputes it the same way
        assert!(matches!(chd.verify().unwrap(), VerifyOutcome::Ok));
    }

    #[test]
    fn progress_adds_up_to_the_input() {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("input.bin");
        let output = dir.path().join("output.chd");
        let data = random_bytes(0x1656_67B1_9E37_79F9, 300_000);
        std::fs::write(&input, &data).unwrap();
        let seen = write(
            &input,
            &output,
            512,
            4096,
            [CODEC_ZLIB, CODEC_NONE, CODEC_NONE, CODEC_NONE],
            None,
            &[],
        );
        assert_eq!(seen.iter().sum::<u64>(), data.len() as u64);
    }

    #[test]
    fn a_failed_run_leaves_nothing_behind() {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("input.bin");
        let output = dir.path().join("output.chd");
        std::fs::write(&input, random_bytes(0x2D35_8DBC_AA77_9FA8, 20_480)).unwrap();
        let error = create(
            &input,
            &output,
            512,
            4096,
            [tag(b"nope"), CODEC_NONE, CODEC_NONE, CODEC_NONE],
            None,
            &[],
            &mut |_| {},
        )
        .unwrap_err();
        assert!(error.to_string().contains("unknown compression"));
        assert!(!output.exists());
        assert!(!dir.path().join("output.chd.part").exists());
    }

    #[test]
    fn a_hard_disk_gets_a_guessed_geometry() {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("input.bin");
        let output = dir.path().join("output.chd");
        let data = random_bytes(0x2F3C_1D7A_5B09_E4C6, 700 * 512);
        std::fs::write(&input, &data).unwrap();
        create_hd(
            &input,
            &output,
            512,
            4096,
            [CODEC_ZLIB, CODEC_NONE, CODEC_NONE, CODEC_NONE],
            None,
            None,
            &mut |_| {},
        )
        .unwrap();

        let mut chd = Chd::open(&output).unwrap();
        assert_eq!(chd.info().logical_size, 358_400);
        let entries = chd.metadata();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].tag, tag(b"GDDD"));
        // 700 sectors: 50 per track, 14 heads, a single cylinder
        assert_eq!(
            entries[0].data,
            b"CYLS:1,HEADS:14,SECS:50,BPS:512\0".to_vec()
        );
        let mut read_back = vec![0u8; data.len()];
        chd.read_bytes(0, &mut read_back).unwrap();
        assert_eq!(read_back, data);
        assert!(matches!(chd.verify().unwrap(), VerifyOutcome::Ok));
    }

    #[test]
    fn a_geometry_can_describe_more_than_the_input() {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("input.bin");
        let output = dir.path().join("output.chd");
        let data = random_bytes(0x6B14_8F2E_90C3_5A77, 20_480);
        std::fs::write(&input, &data).unwrap();
        create_hd(
            &input,
            &output,
            512,
            4096,
            [CODEC_ZLIB, CODEC_NONE, CODEC_NONE, CODEC_NONE],
            None,
            Some((2, 16, 63)),
            &mut |_| {},
        )
        .unwrap();

        // two cylinders of sixteen heads of sixty-three sectors, while the
        // input only fills its first five — the rest reads back as zeros
        let total: usize = 2 * 16 * 63 * 512;
        let mut chd = Chd::open(&output).unwrap();
        assert_eq!(chd.info().logical_size, total as u64);
        let mut read_back = vec![0xAAu8; total];
        chd.read_bytes(0, &mut read_back).unwrap();
        assert_eq!(&read_back[..data.len()], &data[..]);
        assert!(read_back[data.len()..].iter().all(|&byte| byte == 0));
        assert!(matches!(chd.verify().unwrap(), VerifyOutcome::Ok));
    }

    #[test]
    fn an_indivisible_image_is_rejected() {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("input.bin");
        let output = dir.path().join("output.chd");
        std::fs::write(&input, random_bytes(0x41AB_77C2_3E50_9D18, 20_481)).unwrap();
        let error = create_hd(
            &input,
            &output,
            512,
            4096,
            [CODEC_ZLIB, CODEC_NONE, CODEC_NONE, CODEC_NONE],
            None,
            None,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("is not divisible by sector size 512")
        );
        assert!(!output.exists());
    }

    #[test]
    fn only_standard_sector_sizes_are_accepted() {
        let dir = TempDir::new().unwrap();
        let input = dir.path().join("input.bin");
        let output = dir.path().join("output.chd");
        std::fs::write(&input, random_bytes(0x0E91_5C43_B8D2_6F0A, 20_480)).unwrap();
        let error = create_hd(
            &input,
            &output,
            1000,
            4096,
            [CODEC_ZLIB, CODEC_NONE, CODEC_NONE, CODEC_NONE],
            None,
            None,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("sector size must be 512, 1024, 2048 or 4096")
        );
        assert!(!output.exists());
    }
}

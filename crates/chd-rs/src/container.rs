//! CHD container: header, metadata, map and raw-hunk reading.
//!
//! A port of the read side of MAME's `src/lib/util/chd.cpp`, covering
//! versions 3, 4 and 5 of the format. CD/DVD/HD/LD are not container
//! variants: they all start with the same 8-byte `MComprHD` magic and are
//! told apart by the metadata tags they carry.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use sha1::{Digest, Sha1};

use crate::bitstream::BitstreamIn;
use crate::codec::{self, Decompressor};
use crate::crc16::crc16;
use crate::error::{Error, Result};
use crate::huffman::HuffmanDecoder;

const MAGIC: &[u8; 8] = b"MComprHD";
const V3_HEADER_SIZE: u32 = 120;
const V4_HEADER_SIZE: u32 = 108;
const V5_HEADER_SIZE: u32 = 124;

// metadata tags, as big-endian fourcc
pub(crate) const MTAG_HARD_DISK: u32 = u32::from_be_bytes(*b"GDDD");
pub(crate) const MTAG_CDROM_OLD: u32 = u32::from_be_bytes(*b"CHCD");
pub(crate) const MTAG_TRACK: u32 = u32::from_be_bytes(*b"CHTR");
pub(crate) const MTAG_TRACK2: u32 = u32::from_be_bytes(*b"CHT2");
pub(crate) const MTAG_GDROM_OLD: u32 = u32::from_be_bytes(*b"CHGT");
pub(crate) const MTAG_GDROM_TRACK: u32 = u32::from_be_bytes(*b"CHGD");
pub(crate) const MTAG_DVD: u32 = u32::from_be_bytes(*b"DVD ");
const MTAG_LD_VIDEO: u32 = u32::from_be_bytes(*b"AVAV");
const MTAG_LD_DISC: u32 = u32::from_be_bytes(*b"AVLD");

pub(crate) const MDFLAGS_CHECKSUM: u8 = 0x01;

// v5 map entry types
const TYPE_0: u8 = 0;
const TYPE_NONE: u8 = 4;
const TYPE_SELF: u8 = 5;
const TYPE_PARENT: u8 = 6;
const TYPE_RLE_SMALL: u32 = 7;
const TYPE_RLE_LARGE: u32 = 8;
const TYPE_SELF_0: u8 = 9;
const TYPE_SELF_1: u8 = 10;
const TYPE_PARENT_SELF: u8 = 11;
const TYPE_PARENT_0: u8 = 12;
const TYPE_PARENT_1: u8 = 13;

// v3/v4 map entry types (low nibble of the flags byte)
const RAW_TYPE_COMPRESSED: u8 = 1;
const RAW_TYPE_UNCOMPRESSED: u8 = 2;
const RAW_TYPE_MINI: u8 = 3;
const RAW_TYPE_SELF_HUNK: u8 = 4;
const RAW_TYPE_PARENT_HUNK: u8 = 5;
const RAW_FLAG_NO_CRC: u8 = 0x10;

/// The logical content of a CHD, told apart from its metadata tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChdType {
    /// An optical-disc image: CD, GD or DVD-Video.
    Cd,
    /// A DVD read-only-memory image.
    Dvd,
    /// A fixed disk image.
    Hd,
    /// A LaserDisc video image.
    Ld,
}

/// A metadata entry of an opened CHD, as stored in its metadata chain.
#[derive(Debug, Clone)]
pub struct MetadataEntry {
    /// The fourcc metadata tag.
    pub tag: u32,
    /// Which entry of this tag this is, counted in file order.
    pub index: u32,
    /// The entry's flags; [`MDFLAGS_CHECKSUM`] marks checksummed entries.
    pub flags: u8,
    /// The entry's payload.
    pub data: Vec<u8>,
}

/// A summary of an opened CHD, like `chdman info`.
#[derive(Debug, Clone)]
pub struct ChdInfo {
    /// The logical content type.
    pub chd_type: ChdType,
    /// The format version, 3, 4 or 5.
    pub version: u32,
    /// The codec slots, as big-endian fourcc tags.
    pub compression: [u32; 4],
    /// The codecs, formatted like `chdman` does.
    pub compression_name: String,
    /// The logical size, in bytes.
    pub logical_size: u64,
    /// The hunk size, in bytes.
    pub hunk_size: u32,
    /// The unit size, in bytes.
    pub unit_size: u32,
    /// The number of hunks.
    pub hunk_count: u64,
    /// The number of units.
    pub unit_count: u64,
    /// The overall SHA-1 of the file, if set.
    pub sha1: Option<[u8; 20]>,
    /// The SHA-1 of the logical data, if set.
    pub data_sha1: Option<[u8; 20]>,
    /// The SHA-1 of the parent CHD, if the file is a clone.
    pub parent_sha1: Option<[u8; 20]>,
    /// The number of tracks, for optical-disc images.
    pub track_count: usize,
}

/// The outcome of [`Chd::verify`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyOutcome {
    /// The CHD is uncompressed, there is nothing to verify.
    Uncompressed,
    /// The CHD carries no data checksum, there is nothing to verify.
    NoChecksum,
    /// The logical data does not hash to the header's data SHA-1.
    RawMismatch {
        /// The SHA-1 the header carries.
        expected: [u8; 20],
        /// The SHA-1 computed from the data.
        actual: [u8; 20],
    },
    /// The file does not hash to the header's overall SHA-1.
    OverallMismatch {
        /// The SHA-1 the header carries, if set.
        expected: Option<[u8; 20]>,
        /// The SHA-1 computed from the file.
        actual: [u8; 20],
    },
    /// Every checksum matched.
    Ok,
}

/// Formats a SHA-1 the way `chdman` prints them.
pub fn sha1_hex(sha1: &[u8; 20]) -> String {
    sha1.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Formats a metadata or codec tag as its fourcc, question marks for the
/// non-printable parts.
pub fn fourcc(tag: u32) -> String {
    codec::fourcc(tag)
}

fn u16be(data: &[u8]) -> u16 {
    u16::from_be_bytes([data[0], data[1]])
}

fn u24be(data: &[u8]) -> u32 {
    u32::from_be_bytes([0, data[0], data[1], data[2]])
}

fn u32be(data: &[u8]) -> u32 {
    u32::from_be_bytes([data[0], data[1], data[2], data[3]])
}

fn u48be(data: &[u8]) -> u64 {
    u64::from_be_bytes([0, 0, data[0], data[1], data[2], data[3], data[4], data[5]])
}

fn u64be(data: &[u8]) -> u64 {
    u64::from_be_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ])
}

fn put_u24be(data: &mut [u8], value: u32) {
    data[0] = (value >> 16) as u8;
    data[1] = (value >> 8) as u8;
    data[2] = value as u8;
}

fn put_u48be(data: &mut [u8], value: u64) {
    data[..6].copy_from_slice(&value.to_be_bytes()[2..]);
}

fn put_u64be(data: &mut [u8], value: u64) {
    data[..8].copy_from_slice(&value.to_be_bytes());
}

fn read_at(file: &mut File, offset: u64, dest: &mut [u8]) -> Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(dest)?;
    Ok(())
}

fn put_u16be(data: &mut [u8], value: u16) {
    data[0] = (value >> 8) as u8;
    data[1] = value as u8;
}

fn read_sha1_at(data: &[u8]) -> Option<[u8; 20]> {
    let sha1: [u8; 20] = data[..20].try_into().unwrap();
    if sha1.iter().all(|byte| *byte == 0) {
        None
    } else {
        Some(sha1)
    }
}

/// The fields the header parsers share.
struct Parsed {
    version: u32,
    compression: [u32; 4],
    logical_bytes: u64,
    map_offset: u64,
    meta_offset: u64,
    hunk_bytes: u32,
    unit_bytes: u32,
    hunk_count: u32,
    unit_count: u64,
    map_entry_bytes: usize,
    sha1: Option<[u8; 20]>,
    raw_sha1: Option<[u8; 20]>,
    parent_sha1: Option<[u8; 20]>,
}

/// An opened CHD file, read side.
pub struct Chd {
    file: File,
    version: u32,
    compression: [u32; 4],
    logical_bytes: u64,
    hunk_bytes: u32,
    unit_bytes: u32,
    hunk_count: u32,
    unit_count: u64,
    map_entry_bytes: usize,
    rawmap: Vec<u8>,
    sha1: Option<[u8; 20]>,
    raw_sha1: Option<[u8; 20]>,
    parent_sha1: Option<[u8; 20]>,
    metadata: Vec<MetadataEntry>,
    parent: Option<Box<Chd>>,
    parent_missing: bool,
    decompressors: [Option<Decompressor>; 4],
    cache: Vec<u8>,
    cachehunk: u32,
}

impl Chd {
    /// Opens a CHD.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_maybe_parent(path.as_ref(), None)
    }

    /// Opens a CHD along with the parent its clone refers to.
    pub fn open_with_parent(path: impl AsRef<Path>, parent: impl AsRef<Path>) -> Result<Self> {
        Self::open_maybe_parent(path.as_ref(), Some(parent.as_ref()))
    }

    fn open_maybe_parent(path: &Path, parent_path: Option<&Path>) -> Result<Self> {
        let mut file = File::open(path)?;
        let parsed = Self::parse_header(&mut file)?;

        // validate the compression tags; unknown fourcc are rejected at
        // open, like chd_file::create_open_common does
        for tag in parsed.compression {
            if tag != codec::CODEC_NONE && !codec::is_known(tag) {
                return Err(Error::Unsupported(format!(
                    "unknown compression `{}`",
                    codec::fourcc(tag)
                )));
            }
        }

        if parsed.hunk_bytes == 0 {
            return Err(Error::Corrupt("invalid hunk size".into()));
        }
        let metadata = Self::load_metadata(&mut file, parsed.meta_offset)?;
        // v3 and v4 headers store no unit size, it is guessed from the
        // metadata
        let (unit_bytes, unit_count) = if parsed.version < 5 {
            let unit_bytes = guess_unitbytes(&metadata, parsed.hunk_bytes);
            let unit_count = if unit_bytes == 0 {
                0
            } else {
                parsed.logical_bytes.div_ceil(u64::from(unit_bytes))
            };
            (unit_bytes, unit_count)
        } else {
            (parsed.unit_bytes, parsed.unit_count)
        };

        let mut chd = Chd {
            file,
            version: parsed.version,
            compression: parsed.compression,
            logical_bytes: parsed.logical_bytes,
            hunk_bytes: parsed.hunk_bytes,
            unit_bytes,
            hunk_count: parsed.hunk_count,
            unit_count,
            map_entry_bytes: parsed.map_entry_bytes,
            rawmap: Vec::new(),
            sha1: parsed.sha1,
            raw_sha1: parsed.raw_sha1,
            parent_sha1: parsed.parent_sha1,
            metadata,
            parent: None,
            parent_missing: false,
            decompressors: [None, None, None, None],
            cache: vec![0u8; parsed.hunk_bytes as usize],
            cachehunk: u32::MAX,
        };

        // resolve the parent, like chd_file::open does
        let parent = match parent_path {
            Some(parent_path) => Some(Box::new(Self::open_maybe_parent(parent_path, None)?)),
            None => None,
        };
        match (&chd.parent_sha1, parent) {
            (Some(sha1), Some(parent)) => {
                if parent.sha1.as_ref() != Some(sha1) {
                    return Err(Error::InvalidOption(
                        "the parent CHD does not match the SHA-1 of the clone".into(),
                    ));
                }
                chd.parent = Some(parent);
            }
            (Some(_), None) => chd.parent_missing = true,
            (None, Some(_)) => {
                return Err(Error::InvalidOption(
                    "a parent CHD was given but this CHD has none".into(),
                ));
            }
            (None, None) => {}
        }

        // read the map
        let entry_bytes = chd.map_entry_bytes;
        let rawmap_len =
            chd.hunk_count
                .checked_mul(entry_bytes as u32)
                .ok_or_else(|| Error::Corrupt("too many hunks".into()))? as usize;
        chd.rawmap = if chd.version == 5 && chd.compressed() {
            decompress_v5_map(
                &mut chd.file,
                parsed.map_offset,
                chd.hunk_count,
                chd.hunk_bytes,
                chd.unit_bytes,
            )?
        } else {
            let mut rawmap = vec![0u8; rawmap_len];
            read_at(&mut chd.file, parsed.map_offset, &mut rawmap)?;
            rawmap
        };

        Ok(chd)
    }

    fn parse_header(file: &mut File) -> Result<Parsed> {
        let mut header = [0u8; V5_HEADER_SIZE as usize];
        // the magic, the header size and the version
        file.seek(SeekFrom::Start(0))?;
        file.read_exact(&mut header[..16])?;
        if header[..MAGIC.len()] != *MAGIC {
            return Err(Error::BadMagic {
                expected: String::from_utf8_lossy(MAGIC).into_owned(),
                found: String::from_utf8_lossy(&header[..MAGIC.len()]).into_owned(),
            });
        }
        let version = u32be(&header[12..]);
        let parsed = match version {
            3 => {
                file.seek(SeekFrom::Start(0))?;
                file.read_exact(&mut header[..V3_HEADER_SIZE as usize])?;
                if u32be(&header[8..]) != V3_HEADER_SIZE {
                    return Err(Error::Corrupt("invalid header size".into()));
                }
                let flags = u32be(&header[16..]);
                let compression = [Self::legacy_codec(u32be(&header[20..]))?, 0, 0, 0];
                let logical_bytes = u64be(&header[28..]);
                let hunk_bytes = u32be(&header[76..]);
                Parsed {
                    version,
                    compression,
                    logical_bytes,
                    map_offset: 120,
                    meta_offset: u64be(&header[36..]),
                    hunk_bytes,
                    unit_bytes: 0,
                    hunk_count: u32be(&header[24..]),
                    unit_count: 0,
                    map_entry_bytes: 16,
                    sha1: read_sha1_at(&header[80..]),
                    raw_sha1: None,
                    parent_sha1: if flags & 1 != 0 {
                        read_sha1_at(&header[100..])
                    } else {
                        None
                    },
                }
            }
            4 => {
                file.seek(SeekFrom::Start(0))?;
                file.read_exact(&mut header[..V4_HEADER_SIZE as usize])?;
                if u32be(&header[8..]) != V4_HEADER_SIZE {
                    return Err(Error::Corrupt("invalid header size".into()));
                }
                let flags = u32be(&header[16..]);
                let compression = [Self::legacy_codec(u32be(&header[20..]))?, 0, 0, 0];
                let logical_bytes = u64be(&header[28..]);
                let hunk_bytes = u32be(&header[44..]);
                Parsed {
                    version,
                    compression,
                    logical_bytes,
                    map_offset: 108,
                    meta_offset: u64be(&header[36..]),
                    hunk_bytes,
                    unit_bytes: 0,
                    hunk_count: u32be(&header[24..]),
                    unit_count: 0,
                    map_entry_bytes: 16,
                    sha1: read_sha1_at(&header[48..]),
                    raw_sha1: read_sha1_at(&header[88..]),
                    parent_sha1: if flags & 1 != 0 {
                        read_sha1_at(&header[68..])
                    } else {
                        None
                    },
                }
            }
            5 => {
                file.seek(SeekFrom::Start(0))?;
                file.read_exact(&mut header)?;
                if u32be(&header[8..]) != V5_HEADER_SIZE {
                    return Err(Error::Corrupt("invalid header size".into()));
                }
                let logical_bytes = u64be(&header[32..]);
                let hunk_bytes = u32be(&header[56..]);
                let unit_bytes = u32be(&header[60..]);
                if hunk_bytes == 0 || unit_bytes == 0 {
                    return Err(Error::Corrupt("invalid hunk or unit size".into()));
                }
                let compression = [
                    u32be(&header[16..]),
                    u32be(&header[20..]),
                    u32be(&header[24..]),
                    u32be(&header[28..]),
                ];
                let compressed = compression[0] != codec::CODEC_NONE;
                Parsed {
                    version,
                    compression,
                    logical_bytes,
                    map_offset: u64be(&header[40..]),
                    meta_offset: u64be(&header[48..]),
                    hunk_bytes,
                    unit_bytes,
                    hunk_count: logical_bytes.div_ceil(u64::from(hunk_bytes)) as u32,
                    unit_count: logical_bytes.div_ceil(u64::from(unit_bytes)),
                    map_entry_bytes: if compressed { 12 } else { 4 },
                    sha1: read_sha1_at(&header[84..]),
                    raw_sha1: read_sha1_at(&header[64..]),
                    parent_sha1: read_sha1_at(&header[104..]),
                }
            }
            _ => {
                return Err(Error::Unsupported(format!("CHD version {version}")));
            }
        };
        Ok(parsed)
    }

    fn legacy_codec(value: u32) -> Result<u32> {
        match value {
            0 => Ok(codec::CODEC_NONE),
            1 | 2 => Ok(codec::CODEC_ZLIB),
            3 => Ok(codec::CODEC_AV_HUFF),
            _ => Err(Error::Unsupported(format!(
                "unknown legacy compression {value}"
            ))),
        }
    }

    pub(crate) fn compressed(&self) -> bool {
        self.compression[0] != codec::CODEC_NONE
    }

    fn decompressor(&mut self, slot: usize) -> Result<&mut Decompressor> {
        if self.decompressors[slot].is_none() {
            self.decompressors[slot] = Some(Decompressor::create(
                self.compression[slot],
                self.hunk_bytes,
            )?);
        }
        Ok(self.decompressors[slot].as_mut().expect("just created"))
    }

    fn load_metadata(file: &mut File, offset: u64) -> Result<Vec<MetadataEntry>> {
        let mut entries = Vec::new();
        let mut indexes: HashMap<u32, u32> = HashMap::new();
        let mut off = offset;
        while off != 0 {
            let mut header = [0u8; 16];
            read_at(file, off, &mut header)?;
            let tag = u32be(&header[0..]);
            let flags = header[4];
            let length = u24be(&header[5..]);
            let next = u64be(&header[8..]);
            if next != 0 && next <= off {
                return Err(Error::Corrupt("corrupt metadata chain".into()));
            }
            // never read past the next entry of the chain
            let length = if next != 0 {
                std::cmp::min(
                    u64::from(length),
                    next.saturating_sub(off.saturating_add(16)),
                ) as u32
            } else {
                length
            };
            let index = indexes.get(&tag).copied().unwrap_or(0);
            indexes.insert(tag, index + 1);
            let mut data = vec![0u8; length as usize];
            if length > 0 {
                read_at(file, off + 16, &mut data)?;
            }
            entries.push(MetadataEntry {
                tag,
                index,
                flags,
                data,
            });
            off = next;
        }
        Ok(entries)
    }

    /// Reads one whole hunk, past the logical end included.
    pub(crate) fn read_hunk(&mut self, hunknum: u32, dest: &mut [u8]) -> Result<()> {
        if hunknum >= self.hunk_count || dest.len() != self.hunk_bytes as usize {
            return Err(Error::Corrupt(format!("invalid hunk {hunknum}")));
        }
        if self.version < 5 {
            let base = hunknum as usize * 16;
            let block_offs = u64be(&self.rawmap[base..]);
            let block_crc = u32be(&self.rawmap[base + 8..]);
            let block_len = (u16be(&self.rawmap[base + 12..]) as u32)
                | (u32::from(self.rawmap[base + 14]) << 16);
            let flags = self.rawmap[base + 15];
            let kind = flags & 0x0f;
            let no_crc = flags & RAW_FLAG_NO_CRC != 0;
            match kind {
                RAW_TYPE_COMPRESSED => {
                    let mut source = vec![0u8; block_len as usize];
                    read_at(&mut self.file, block_offs, &mut source)?;
                    self.decompressor(0)?.decompress(&source, dest)?;
                }
                RAW_TYPE_UNCOMPRESSED => {
                    read_at(&mut self.file, block_offs, dest)?;
                }
                RAW_TYPE_MINI => {
                    // a mini hunk is a 64-bit pattern repeated over the hunk
                    if dest.len() >= 8 {
                        put_u64be(dest, block_offs);
                        for b in 8..dest.len() {
                            dest[b] = dest[b - 8];
                        }
                    }
                }
                RAW_TYPE_SELF_HUNK => return self.read_hunk(block_offs as u32, dest),
                RAW_TYPE_PARENT_HUNK => {
                    if self.parent_missing {
                        return Err(Error::Unsupported("CHD requires its parent file".into()));
                    }
                    if let Some(parent) = self.parent.as_mut() {
                        return parent.read_hunk(block_offs as u32, dest);
                    }
                    return Err(Error::Unsupported("CHD requires its parent file".into()));
                }
                _ => return Err(Error::Corrupt(format!("invalid data in hunk {hunknum}"))),
            }
            if !no_crc && crc32fast::hash(dest) != block_crc {
                return Err(Error::Corrupt(format!(
                    "hunk {hunknum} failed its CRC check"
                )));
            }
            return Ok(());
        }
        if !self.compressed() {
            let block_offs = u32be(&self.rawmap[hunknum as usize * 4..]);
            if block_offs == 0 {
                if self.parent_missing {
                    return Err(Error::Unsupported("CHD requires its parent file".into()));
                }
                if let Some(parent) = self.parent.as_mut() {
                    return parent.read_hunk(hunknum, dest);
                }
                dest.fill(0);
            } else {
                read_at(
                    &mut self.file,
                    u64::from(block_offs) * u64::from(self.hunk_bytes),
                    dest,
                )?;
            }
            return Ok(());
        }
        let base = hunknum as usize * 12;
        let kind = self.rawmap[base];
        let block_len = u24be(&self.rawmap[base + 1..]) as usize;
        let block_offs = u48be(&self.rawmap[base + 4..]);
        let block_crc = u16be(&self.rawmap[base + 10..]);
        match kind {
            TYPE_0..=3 => {
                let mut source = vec![0u8; block_len];
                read_at(&mut self.file, block_offs, &mut source)?;
                self.decompressor(usize::from(kind))?
                    .decompress(&source, dest)?;
            }
            TYPE_NONE => {
                read_at(&mut self.file, block_offs, dest)?;
            }
            TYPE_SELF => return self.read_hunk(block_offs as u32, dest),
            TYPE_PARENT => {
                if self.parent_missing {
                    return Err(Error::Unsupported("CHD requires its parent file".into()));
                }
                if let Some(parent) = self.parent.as_mut() {
                    let unit_bytes = u64::from(parent.unit_bytes);
                    return parent.read_bytes(block_offs * unit_bytes, dest);
                }
                return Err(Error::Unsupported("CHD requires its parent file".into()));
            }
            _ => return Err(Error::Corrupt(format!("invalid data in hunk {hunknum}"))),
        }
        if crc16(dest) != block_crc {
            return Err(Error::Corrupt(format!(
                "hunk {hunknum} failed its CRC check"
            )));
        }
        Ok(())
    }

    /// Reads `dest.len()` bytes of the logical data, starting at `offset`.
    pub fn read_bytes(&mut self, offset: u64, dest: &mut [u8]) -> Result<()> {
        if dest.is_empty() {
            return Ok(());
        }
        if offset
            .checked_add(dest.len() as u64)
            .is_none_or(|end| end > self.logical_bytes)
        {
            return Err(Error::InvalidOption("read past end of CHD".into()));
        }
        let hunk_bytes = u64::from(self.hunk_bytes);
        let mut pos = offset;
        let mut done = 0usize;
        while done < dest.len() {
            let hunk = (pos / hunk_bytes) as u32;
            let start = (pos % hunk_bytes) as usize;
            let length =
                std::cmp::min(hunk_bytes - start as u64, (dest.len() - done) as u64) as usize;
            if start == 0 && length == self.cache.len() && hunk != self.cachehunk {
                self.read_hunk(hunk, &mut dest[done..done + length])?;
            } else {
                if hunk != self.cachehunk {
                    let mut cache = std::mem::take(&mut self.cache);
                    let result = self.read_hunk(hunk, &mut cache);
                    self.cache = cache;
                    result?;
                    self.cachehunk = hunk;
                }
                dest[done..done + length].copy_from_slice(&self.cache[start..start + length]);
            }
            done += length;
            pos += length as u64;
        }
        Ok(())
    }

    /// Recomputes the overall SHA-1 from the raw data SHA-1 and the
    /// checksummed metadata, like `compute_overall_sha1` does.
    pub fn compute_overall_sha1(&self, raw_sha1: &[u8; 20]) -> [u8; 20] {
        if self.version < 4 {
            return *raw_sha1;
        }
        let mut hashes: Vec<[u8; 24]> = Vec::new();
        for entry in &self.metadata {
            if entry.flags & MDFLAGS_CHECKSUM == 0 {
                continue;
            }
            let mut hash = [0u8; 24];
            hash[0..4].copy_from_slice(&entry.tag.to_be_bytes());
            hash[4..24].copy_from_slice(&Sha1::digest(&entry.data));
            hashes.push(hash);
        }
        hashes.sort();
        let mut hasher = Sha1::new();
        hasher.update(raw_sha1);
        for hash in &hashes {
            hasher.update(hash);
        }
        hasher.finalize().into()
    }

    /// Verifies the CHD against the checksums of its header.
    pub fn verify(&mut self) -> Result<VerifyOutcome> {
        self.verify_with(&mut |_| {})
    }

    /// Same as [`Self::verify`], reporting the logical bytes checked since
    /// the previous call.
    pub fn verify_with(&mut self, progress: &mut dyn FnMut(u64)) -> Result<VerifyOutcome> {
        if !self.compressed() {
            return Ok(VerifyOutcome::Uncompressed);
        }
        let reference = if self.version <= 3 {
            self.sha1
        } else {
            self.raw_sha1
        };
        let Some(reference) = reference else {
            return Ok(VerifyOutcome::NoChecksum);
        };
        let mut hasher = Sha1::new();
        let block_len = std::cmp::max(1 << 20, self.hunk_bytes as usize);
        let mut buffer = vec![0u8; block_len];
        let mut pos = 0u64;
        while pos < self.logical_bytes {
            let length = std::cmp::min(block_len as u64, self.logical_bytes - pos) as usize;
            self.read_bytes(pos, &mut buffer[..length])?;
            hasher.update(&buffer[..length]);
            pos += length as u64;
            progress(length as u64);
        }
        let computed: [u8; 20] = hasher.finalize().into();
        if computed != reference {
            return Ok(VerifyOutcome::RawMismatch {
                expected: reference,
                actual: computed,
            });
        }
        if self.version >= 4 {
            let overall = self.compute_overall_sha1(&computed);
            if self.sha1 != Some(overall) {
                return Ok(VerifyOutcome::OverallMismatch {
                    expected: self.sha1,
                    actual: overall,
                });
            }
        }
        Ok(VerifyOutcome::Ok)
    }

    /// Extracts a DVD or hard disk CHD to a raw image.
    ///
    /// Like [`Self::verify_with`], reports the logical bytes written since
    /// the previous call. The output lands on `<output>.part` and is renamed
    /// into place once complete; a failed run leaves nothing behind.
    pub fn extract(&mut self, output: &Path, progress: &mut dyn FnMut(u64)) -> Result<()> {
        match self.detect_type() {
            ChdType::Cd | ChdType::Ld => {
                return Err(Error::Unsupported(
                    "CD and LD extraction needs the frame layout of a later version".to_string(),
                ));
            }
            ChdType::Dvd | ChdType::Hd => {}
        }
        let part = crate::writer::part_path(output);
        let result = self.extract_to(&part, progress);
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

    fn extract_to(&mut self, part: &Path, progress: &mut dyn FnMut(u64)) -> Result<()> {
        use std::io::Write;
        let mut file = std::fs::File::create(part)?;
        let block_len = std::cmp::max(1 << 20, self.hunk_bytes as usize);
        let mut buffer = vec![0u8; block_len];
        let mut pos = 0u64;
        while pos < self.logical_bytes {
            let length = std::cmp::min(block_len as u64, self.logical_bytes - pos) as usize;
            self.read_bytes(pos, &mut buffer[..length])?;
            file.write_all(&buffer[..length])?;
            pos += length as u64;
            progress(length as u64);
        }
        file.flush()?;
        Ok(())
    }

    /// Returns a summary of the CHD, like `chdman info`.
    pub fn info(&self) -> ChdInfo {
        ChdInfo {
            chd_type: self.detect_type(),
            version: self.version,
            compression: self.compression,
            compression_name: codec::compression_string(&self.compression),
            logical_size: self.logical_bytes,
            hunk_size: self.hunk_bytes,
            unit_size: self.unit_bytes,
            hunk_count: u64::from(self.hunk_count),
            unit_count: self.unit_count,
            sha1: self.sha1,
            data_sha1: self.raw_sha1,
            parent_sha1: self.parent_sha1,
            track_count: self.track_count(),
        }
    }

    /// Returns the metadata entries, in the order of the metadata chain.
    pub fn metadata(&self) -> &[MetadataEntry] {
        &self.metadata
    }

    fn detect_type(&self) -> ChdType {
        let has = |tags: &[u32]| self.metadata.iter().any(|entry| tags.contains(&entry.tag));
        if has(&[
            MTAG_CDROM_OLD,
            MTAG_TRACK,
            MTAG_TRACK2,
            MTAG_GDROM_OLD,
            MTAG_GDROM_TRACK,
        ]) {
            ChdType::Cd
        } else if has(&[MTAG_LD_VIDEO, MTAG_LD_DISC]) {
            ChdType::Ld
        } else if has(&[MTAG_DVD]) {
            ChdType::Dvd
        } else {
            ChdType::Hd
        }
    }

    /// The tracks a CD's table of contents holds, whichever of its metadata
    /// forms describes it; 0 for anything but a CD.
    fn track_count(&self) -> usize {
        if self.detect_type() != ChdType::Cd {
            return 0;
        }
        crate::cdrom::toc_from_chd(self).map_or(0, |toc| toc.numtrks)
    }
}

/// v3 and v4 headers carry no unit size; guess it from the metadata, like
/// `chd_file::guess_unitbytes` does.
fn guess_unitbytes(metadata: &[MetadataEntry], hunk_bytes: u32) -> u32 {
    if let Some(entry) = metadata
        .iter()
        .find(|entry| entry.tag == MTAG_HARD_DISK && entry.index == 0)
    {
        // HARD_DISK_METADATA_FORMAT is "%d,%d,%d,%d", the last being the
        // unit size; sscanf leaves trailing junk alone, so take only the
        // leading digits.
        if let Ok(text) = std::str::from_utf8(&entry.data) {
            let fields: Vec<&str> = text.split(',').collect();
            if fields.len() >= 4 {
                let last = fields[3].trim();
                let digits = &last[..last
                    .find(|c: char| !c.is_ascii_digit())
                    .unwrap_or(last.len())];
                if let Ok(unit_bytes) = digits.parse::<u32>()
                    && unit_bytes > 0
                {
                    return unit_bytes;
                }
            }
        }
    }
    if metadata.iter().any(|entry| {
        entry.index == 0
            && matches!(
                entry.tag,
                MTAG_CDROM_OLD | MTAG_TRACK | MTAG_TRACK2 | MTAG_GDROM_OLD | MTAG_GDROM_TRACK
            )
    }) {
        return codec::FRAME_SIZE as u32;
    }
    hunk_bytes
}

/// Expands the compressed form of a version 5 map into the same 12-byte
/// entries the uncompressed form stores, like `chd_file::decompress_v5_map`
/// does.
fn decompress_v5_map(
    file: &mut File,
    map_offset: u64,
    hunk_count: u32,
    hunk_bytes: u32,
    unit_bytes: u32,
) -> Result<Vec<u8>> {
    let mut rawmap = vec![0u8; hunk_count as usize * 12];
    if map_offset == 0 {
        rawmap.iter_mut().for_each(|byte| *byte = 0xff);
        return Ok(rawmap);
    }
    let mut header = [0u8; 16];
    read_at(file, map_offset, &mut header)?;
    let map_bytes = u32be(&header[0..]) as usize;
    let first_offs = u48be(&header[4..]);
    let map_crc = u16be(&header[10..]);
    let length_bits = u32::from(header[12]);
    let self_bits = u32::from(header[13]);
    let parent_bits = u32::from(header[14]);
    let mut source = vec![0u8; map_bytes];
    read_at(file, map_offset + 16, &mut source)?;
    let mut bitbuf = BitstreamIn::new(&source);
    let mut decoder = HuffmanDecoder::new(16, 8);
    decoder.import_tree_rle(&mut bitbuf)?;

    // first pass: expand the entry types, resolving the run-length codes
    let mut last_comp = 0u8;
    let mut rep_count: i64 = 0;
    for hunk in rawmap.chunks_mut(12) {
        if rep_count > 0 {
            hunk[0] = last_comp;
            rep_count -= 1;
            continue;
        }
        let val = decoder.decode_one(&mut bitbuf);
        match val {
            TYPE_RLE_SMALL => {
                hunk[0] = last_comp;
                rep_count = 2 + i64::from(decoder.decode_one(&mut bitbuf));
            }
            TYPE_RLE_LARGE => {
                rep_count = 2 + 16 + (i64::from(decoder.decode_one(&mut bitbuf)) << 4);
                rep_count += i64::from(decoder.decode_one(&mut bitbuf));
                hunk[0] = last_comp;
            }
            _ => {
                last_comp = val as u8;
                hunk[0] = last_comp;
            }
        }
    }

    // second pass: fill in the offset, length and CRC of each entry
    let mut cur_offset = first_offs;
    let mut last_self = 0u32;
    let mut last_parent = 0u64;
    for (hunknum, hunk) in rawmap.chunks_mut(12).enumerate() {
        let mut offset = cur_offset;
        let mut length = 0u32;
        let mut crc = 0u16;
        match hunk[0] {
            TYPE_0..=3 => {
                length = bitbuf.read(length_bits);
                cur_offset += u64::from(length);
                crc = bitbuf.read(16) as u16;
            }
            TYPE_NONE => {
                length = hunk_bytes;
                cur_offset += u64::from(length);
                crc = bitbuf.read(16) as u16;
            }
            TYPE_SELF => {
                last_self = bitbuf.read(self_bits);
                offset = u64::from(last_self);
            }
            TYPE_PARENT => {
                offset = u64::from(bitbuf.read(parent_bits));
                last_parent = offset;
            }
            TYPE_SELF_1 => {
                last_self += 1;
                hunk[0] = TYPE_SELF;
                offset = u64::from(last_self);
            }
            TYPE_SELF_0 => {
                hunk[0] = TYPE_SELF;
                offset = u64::from(last_self);
            }
            TYPE_PARENT_SELF => {
                last_parent = u64::from((hunknum as u32).wrapping_mul(hunk_bytes) / unit_bytes);
                hunk[0] = TYPE_PARENT;
                offset = last_parent;
            }
            TYPE_PARENT_1 => {
                last_parent += u64::from(hunk_bytes / unit_bytes);
                hunk[0] = TYPE_PARENT;
                offset = last_parent;
            }
            TYPE_PARENT_0 => {
                hunk[0] = TYPE_PARENT;
                offset = last_parent;
            }
            _ => {}
        }
        put_u24be(&mut hunk[1..], length);
        put_u48be(&mut hunk[4..], offset);
        put_u16be(&mut hunk[10..], crc);
    }
    if crc16(&rawmap) != map_crc {
        return Err(Error::Corrupt("map failed its CRC check".into()));
    }
    Ok(rawmap)
}

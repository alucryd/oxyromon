//! RVZ's on-disk structures, all big endian. See Dolphin's
//! `docs/WiaAndRvz.md`.

use crate::crypto::Key;
use crate::error::{Error, Result};

pub const MAGIC: [u8; 4] = *b"RVZ\x01";
pub const VERSION: u32 = 0x01000000;
pub const VERSION_WRITE_COMPATIBLE: u32 = 0x00030000;
pub const VERSION_READ_COMPATIBLE: u32 = 0x00030000;

pub const HEADER_1_SIZE: usize = 0x48;
pub const HEADER_2_SIZE: usize = 0xDC;
pub const DISC_HEADER_SIZE: usize = 0x80;
pub const PARTITION_ENTRY_SIZE: usize = 0x30;
pub const RAW_DATA_ENTRY_SIZE: usize = 0x18;
pub const GROUP_ENTRY_SIZE: usize = 0x0C;
pub const EXCEPTION_SIZE: usize = 0x16;

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes(bytes[at..at + 2].try_into().unwrap())
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_be_bytes(bytes[at..at + 8].try_into().unwrap())
}

#[derive(Clone, Debug, Default)]
pub struct Header1 {
    pub magic: [u8; 4],
    pub version: u32,
    pub version_compatible: u32,
    pub header_2_size: u32,
    pub header_2_hash: [u8; 20],
    pub iso_file_size: u64,
    pub file_size: u64,
    pub header_1_hash: [u8; 20],
}

impl Header1 {
    pub fn parse(bytes: &[u8]) -> Self {
        Self {
            magic: bytes[0..4].try_into().unwrap(),
            version: u32_at(bytes, 4),
            version_compatible: u32_at(bytes, 8),
            header_2_size: u32_at(bytes, 12),
            header_2_hash: bytes[16..36].try_into().unwrap(),
            iso_file_size: u64_at(bytes, 36),
            file_size: u64_at(bytes, 44),
            header_1_hash: bytes[52..72].try_into().unwrap(),
        }
    }

    pub fn to_bytes(&self) -> [u8; HEADER_1_SIZE] {
        let mut out = [0; HEADER_1_SIZE];
        out[0..4].copy_from_slice(&self.magic);
        out[4..8].copy_from_slice(&self.version.to_be_bytes());
        out[8..12].copy_from_slice(&self.version_compatible.to_be_bytes());
        out[12..16].copy_from_slice(&self.header_2_size.to_be_bytes());
        out[16..36].copy_from_slice(&self.header_2_hash);
        out[36..44].copy_from_slice(&self.iso_file_size.to_be_bytes());
        out[44..52].copy_from_slice(&self.file_size.to_be_bytes());
        out[52..72].copy_from_slice(&self.header_1_hash);
        out
    }
}

#[derive(Clone, Debug)]
pub struct Header2 {
    pub disc_type: u32,
    pub compression_type: u32,
    pub compression_level: i32,
    pub chunk_size: u32,
    pub disc_header: [u8; DISC_HEADER_SIZE],
    pub partition_entries: u32,
    pub partition_entry_size: u32,
    pub partition_entries_offset: u64,
    pub partition_entries_hash: [u8; 20],
    pub raw_data_entries: u32,
    pub raw_data_entries_offset: u64,
    pub raw_data_entries_size: u32,
    pub group_entries: u32,
    pub group_entries_offset: u64,
    pub group_entries_size: u32,
    pub compressor_data_size: u8,
    pub compressor_data: [u8; 7],
}

impl Default for Header2 {
    fn default() -> Self {
        Self {
            disc_type: 0,
            compression_type: 0,
            compression_level: 0,
            chunk_size: 0,
            disc_header: [0; DISC_HEADER_SIZE],
            partition_entries: 0,
            partition_entry_size: 0,
            partition_entries_offset: 0,
            partition_entries_hash: [0; 20],
            raw_data_entries: 0,
            raw_data_entries_offset: 0,
            raw_data_entries_size: 0,
            group_entries: 0,
            group_entries_offset: 0,
            group_entries_size: 0,
            compressor_data_size: 0,
            compressor_data: [0; 7],
        }
    }
}

impl Header2 {
    /// Parse a header that may be shorter than this version's, its missing
    /// end read as zeros.
    pub fn parse(bytes: &[u8]) -> Self {
        let mut full = [0; HEADER_2_SIZE];
        let length = bytes.len().min(HEADER_2_SIZE);
        full[..length].copy_from_slice(&bytes[..length]);
        let b = &full;
        Self {
            disc_type: u32_at(b, 0),
            compression_type: u32_at(b, 4),
            compression_level: u32_at(b, 8) as i32,
            chunk_size: u32_at(b, 12),
            disc_header: b[16..144].try_into().unwrap(),
            partition_entries: u32_at(b, 144),
            partition_entry_size: u32_at(b, 148),
            partition_entries_offset: u64_at(b, 152),
            partition_entries_hash: b[160..180].try_into().unwrap(),
            raw_data_entries: u32_at(b, 180),
            raw_data_entries_offset: u64_at(b, 184),
            raw_data_entries_size: u32_at(b, 192),
            group_entries: u32_at(b, 196),
            group_entries_offset: u64_at(b, 200),
            group_entries_size: u32_at(b, 208),
            compressor_data_size: b[212],
            compressor_data: b[213..220].try_into().unwrap(),
        }
    }

    pub fn to_bytes(&self) -> [u8; HEADER_2_SIZE] {
        let mut out = [0; HEADER_2_SIZE];
        out[0..4].copy_from_slice(&self.disc_type.to_be_bytes());
        out[4..8].copy_from_slice(&self.compression_type.to_be_bytes());
        out[8..12].copy_from_slice(&self.compression_level.to_be_bytes());
        out[12..16].copy_from_slice(&self.chunk_size.to_be_bytes());
        out[16..144].copy_from_slice(&self.disc_header);
        out[144..148].copy_from_slice(&self.partition_entries.to_be_bytes());
        out[148..152].copy_from_slice(&self.partition_entry_size.to_be_bytes());
        out[152..160].copy_from_slice(&self.partition_entries_offset.to_be_bytes());
        out[160..180].copy_from_slice(&self.partition_entries_hash);
        out[180..184].copy_from_slice(&self.raw_data_entries.to_be_bytes());
        out[184..192].copy_from_slice(&self.raw_data_entries_offset.to_be_bytes());
        out[192..196].copy_from_slice(&self.raw_data_entries_size.to_be_bytes());
        out[196..200].copy_from_slice(&self.group_entries.to_be_bytes());
        out[200..208].copy_from_slice(&self.group_entries_offset.to_be_bytes());
        out[208..212].copy_from_slice(&self.group_entries_size.to_be_bytes());
        out[212] = self.compressor_data_size;
        out[213..220].copy_from_slice(&self.compressor_data);
        out
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PartitionDataEntry {
    pub first_sector: u32,
    pub sectors: u32,
    pub group_index: u32,
    pub groups: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PartitionEntry {
    pub key: Key,
    pub data: [PartitionDataEntry; 2],
}

impl PartitionEntry {
    pub fn parse(bytes: &[u8]) -> Self {
        let data = |at: usize| PartitionDataEntry {
            first_sector: u32_at(bytes, at),
            sectors: u32_at(bytes, at + 4),
            group_index: u32_at(bytes, at + 8),
            groups: u32_at(bytes, at + 12),
        };
        Self {
            key: bytes[0..16].try_into().unwrap(),
            data: [data(16), data(32)],
        }
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.key);
        for data in &self.data {
            out.extend_from_slice(&data.first_sector.to_be_bytes());
            out.extend_from_slice(&data.sectors.to_be_bytes());
            out.extend_from_slice(&data.group_index.to_be_bytes());
            out.extend_from_slice(&data.groups.to_be_bytes());
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RawDataEntry {
    pub data_offset: u64,
    pub data_size: u64,
    pub group_index: u32,
    pub groups: u32,
}

impl RawDataEntry {
    pub fn parse(bytes: &[u8]) -> Self {
        Self {
            data_offset: u64_at(bytes, 0),
            data_size: u64_at(bytes, 8),
            group_index: u32_at(bytes, 16),
            groups: u32_at(bytes, 20),
        }
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.data_offset.to_be_bytes());
        out.extend_from_slice(&self.data_size.to_be_bytes());
        out.extend_from_slice(&self.group_index.to_be_bytes());
        out.extend_from_slice(&self.groups.to_be_bytes());
    }
}

/// Where a group's data is stored. The top bit of `data_size` says whether
/// it is compressed; its other bits at 0 mean it is all zeros.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GroupEntry {
    /// The offset in the file, divided by 4.
    pub data_offset: u32,
    pub data_size: u32,
    /// The size of the RVZ packed data, 0 if it is not packed.
    pub packed_size: u32,
}

impl GroupEntry {
    pub fn parse(bytes: &[u8]) -> Self {
        Self {
            data_offset: u32_at(bytes, 0),
            data_size: u32_at(bytes, 4),
            packed_size: u32_at(bytes, 8),
        }
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.data_offset.to_be_bytes());
        out.extend_from_slice(&self.data_size.to_be_bytes());
        out.extend_from_slice(&self.packed_size.to_be_bytes());
    }
}

/// A hash to replace a recomputed one with: an offset among a chunk's
/// hashes, and the hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exception {
    pub offset: u16,
    pub hash: [u8; 20],
}

impl Exception {
    pub fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.offset.to_be_bytes());
        out.extend_from_slice(&self.hash);
    }
}

/// Parse `count` exception lists from the start of `bytes`, returning them
/// and how many bytes they took.
pub fn parse_exception_lists(bytes: &[u8], count: u32) -> Result<(Vec<Vec<Exception>>, usize)> {
    let short = || Error::Corrupt("truncated hash exceptions".into());
    let mut lists = Vec::with_capacity(count as usize);
    let mut used = 0;
    for _ in 0..count {
        if used + 2 > bytes.len() {
            return Err(short());
        }
        let exceptions = u16_at(bytes, used) as usize;
        used += 2;
        if used + exceptions * EXCEPTION_SIZE > bytes.len() {
            return Err(short());
        }
        let list = (0..exceptions)
            .map(|i| {
                let at = used + i * EXCEPTION_SIZE;
                Exception {
                    offset: u16_at(bytes, at),
                    hash: bytes[at + 2..at + EXCEPTION_SIZE].try_into().unwrap(),
                }
            })
            .collect();
        used += exceptions * EXCEPTION_SIZE;
        lists.push(list);
    }
    Ok((lists, used))
}

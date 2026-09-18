//! NCZ header structures: the section table and the optional block header.
//!
//! Layout at offset 0x4000 of an NCZ file:
//!   "NCZSECTN" (8) | sectionCount: i64 | sectionCount x Section
//!   optionally "NCZBLOCK" | BlockHeader
//!
//! Mirrors `nsz/Header.py` and `docs/formats.md`.

use crate::error::{Error, Result};

pub const SECTN_MAGIC: &[u8; 8] = b"NCZSECTN";
pub const BLOCK_MAGIC: &[u8; 8] = b"NCZBLOCK";
/// The first 0x4000 bytes of an NCA are stored verbatim in an NCZ.
pub const INCOMPRESSIBLE_HEADER_SIZE: u64 = 0x4000;

/// One entry in the NCZ section table (64 bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub offset: i64,
    pub size: i64,
    pub crypto_type: i64,
    pub crypto_key: [u8; 16],
    pub crypto_counter: [u8; 16],
}

impl Section {
    pub const WIRE_SIZE: usize = 64;

    pub fn read(buf: &[u8]) -> Result<Section> {
        if buf.len() < Self::WIRE_SIZE {
            return Err(Error::Corrupt("section too short".into()));
        }
        let mut crypto_key = [0u8; 16];
        let mut crypto_counter = [0u8; 16];
        crypto_key.copy_from_slice(&buf[32..48]);
        crypto_counter.copy_from_slice(&buf[48..64]);
        Ok(Section {
            offset: le_i64(buf, 0),
            size: le_i64(buf, 8),
            crypto_type: le_i64(buf, 16),
            crypto_key,
            crypto_counter,
            // buf[24..32] is padding, ignored
        })
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.offset.to_le_bytes());
        out.extend_from_slice(&self.size.to_le_bytes());
        out.extend_from_slice(&self.crypto_type.to_le_bytes());
        out.extend_from_slice(&0i64.to_le_bytes()); // padding
        out.extend_from_slice(&self.crypto_key);
        out.extend_from_slice(&self.crypto_counter);
    }
}

/// The optional NCZ block-compression header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockHeader {
    pub version: i8,
    pub block_type: i8,
    pub unused: i8,
    pub block_size_exponent: i8,
    pub number_of_blocks: i32,
    pub decompressed_size: i64,
    pub compressed_block_size_list: Vec<u32>,
}

impl BlockHeader {
    /// Fixed part before the size list: magic(8)+ver(1)+type(1)+unused(1)+exp(1)+nblocks(4)+dsize(8) = 24
    pub const FIXED_SIZE: usize = 24;

    pub fn read(buf: &[u8]) -> Result<BlockHeader> {
        if buf.len() < Self::FIXED_SIZE {
            return Err(Error::Corrupt("block header too short".into()));
        }
        if &buf[0..8] != BLOCK_MAGIC {
            return Err(Error::BadMagic {
                expected: "NCZBLOCK".into(),
                found: hex::encode(&buf[0..8]),
            });
        }
        let number_of_blocks = le_i32(buf, 12);
        if number_of_blocks < 0 {
            return Err(Error::Corrupt("negative block count".into()));
        }
        let n = number_of_blocks as usize;
        let need = Self::FIXED_SIZE + n * 4;
        if buf.len() < need {
            return Err(Error::Corrupt("block size list truncated".into()));
        }
        let mut list = Vec::with_capacity(n);
        for i in 0..n {
            list.push(le_u32(buf, Self::FIXED_SIZE + i * 4));
        }
        Ok(BlockHeader {
            version: buf[8] as i8,
            block_type: buf[9] as i8,
            unused: buf[10] as i8,
            block_size_exponent: buf[11] as i8,
            number_of_blocks,
            decompressed_size: le_i64(buf, 16),
            compressed_block_size_list: list,
        })
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(BLOCK_MAGIC);
        out.push(self.version as u8);
        out.push(self.block_type as u8);
        out.push(self.unused as u8);
        out.push(self.block_size_exponent as u8);
        out.extend_from_slice(&self.number_of_blocks.to_le_bytes());
        out.extend_from_slice(&self.decompressed_size.to_le_bytes());
        for &s in &self.compressed_block_size_list {
            out.extend_from_slice(&s.to_le_bytes());
        }
    }
}

/// Parse the section table (and optional block header) starting at `buf` (which
/// begins at the NCZ header, i.e. file offset 0x4000).
pub fn parse_header(buf: &[u8]) -> Result<(Vec<Section>, Option<BlockHeader>)> {
    if buf.len() < 16 || &buf[0..8] != SECTN_MAGIC {
        return Err(Error::BadMagic {
            expected: "NCZSECTN".into(),
            found: hex::encode(&buf[0..buf.len().min(8)]),
        });
    }
    let section_count = le_i64(buf, 8);
    if section_count < 0 {
        return Err(Error::Corrupt("negative section count".into()));
    }
    let n = section_count as usize;
    if n > (buf.len() - 16) / Section::WIRE_SIZE {
        return Err(Error::Corrupt("section table truncated".into()));
    }
    let table_end = 16 + n * Section::WIRE_SIZE;
    let mut sections = Vec::with_capacity(n);
    for i in 0..n {
        sections.push(Section::read(&buf[16 + i * Section::WIRE_SIZE..])?);
    }
    let block = if buf.len() >= table_end + 8 && &buf[table_end..table_end + 8] == BLOCK_MAGIC {
        Some(BlockHeader::read(&buf[table_end..])?)
    } else {
        None
    };
    Ok((sections, block))
}

/// Serialize a full NCZ header (section table + optional block header).
pub fn write_header(sections: &[Section], block: Option<&BlockHeader>) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(SECTN_MAGIC);
    out.extend_from_slice(&(sections.len() as i64).to_le_bytes());
    for s in sections {
        s.write(&mut out);
    }
    if let Some(b) = block {
        b.write(&mut out);
    }
    out
}

// --- little-endian scalar readers over byte slices ---
#[inline]
pub fn le_i64(b: &[u8], at: usize) -> i64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[at..at + 8]);
    i64::from_le_bytes(a)
}
#[inline]
pub fn le_i32(b: &[u8], at: usize) -> i32 {
    let mut a = [0u8; 4];
    a.copy_from_slice(&b[at..at + 4]);
    i32::from_le_bytes(a)
}
#[inline]
pub fn le_u32(b: &[u8], at: usize) -> u32 {
    let mut a = [0u8; 4];
    a.copy_from_slice(&b[at..at + 4]);
    u32::from_le_bytes(a)
}

//! The WUX header, as wudd writes it and Cemu reads it: what [`Image`]
//! needs to read a WUX, the container itself being wux-rs's.
//!
//! [`Image`]: super::image::Image

use super::error::{Error, Result, corrupt};

/// "WUX0" then `0x1099d02e`, both little-endian.
pub const MAGIC: [u8; 8] = [b'W', b'U', b'X', b'0', 0x2e, 0xd0, 0x99, 0x10];
/// The header's size, where the sector table starts.
pub const HEADER_SIZE: u64 = 32;
/// The sector size wudd writes, a Wii U disc's own.
pub const SECTOR_SIZE: u32 = 0x8000;

/// A WUX header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub sector_size: u32,
    pub uncompressed_size: u64,
    pub flags: u32,
}

impl Header {
    pub fn parse(bytes: &[u8; HEADER_SIZE as usize]) -> Result<Header> {
        if bytes[..8] != MAGIC {
            return Err(Error::BadMagic {
                expected: "WUX0".into(),
                found: String::from_utf8_lossy(&bytes[..4]).into_owned(),
            });
        }
        let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let header = Header {
            sector_size: u32_at(8),
            uncompressed_size: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
            flags: u32_at(24),
        };
        // Cemu's bounds.
        if !(0x100..0x1000_0000).contains(&header.sector_size) {
            return Err(corrupt(format!(
                "the WUX sector size is {}",
                header.sector_size
            )));
        }
        Ok(header)
    }

    pub fn to_bytes(self) -> [u8; HEADER_SIZE as usize] {
        let mut bytes = [0; HEADER_SIZE as usize];
        bytes[..8].copy_from_slice(&MAGIC);
        bytes[8..12].copy_from_slice(&self.sector_size.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.uncompressed_size.to_le_bytes());
        bytes[24..28].copy_from_slice(&self.flags.to_le_bytes());
        bytes
    }

    /// How many sectors the disc has, the last one possibly short.
    pub fn sector_count(&self) -> u64 {
        self.uncompressed_size.div_ceil(u64::from(self.sector_size))
    }

    /// Where the stored sectors start: past the table, aligned to a sector.
    pub fn data_offset(&self) -> u64 {
        (HEADER_SIZE + self.sector_count() * 4).next_multiple_of(u64::from(self.sector_size))
    }
}

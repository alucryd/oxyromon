//! On-disk layout of the CSO and ZSO formats.
//!
//! Both share one container: a 24-byte header, an index of little-endian
//! `u32` offsets, then the block data. They differ only in magic and in which
//! codec a compressed block holds.

use crate::error::{Error, Result};

/// A sector is the unit readers work in; block sizes are multiples of it.
pub const SECTOR_SIZE: u32 = 2048;
/// Largest block size maxcso accepts.
pub const MAX_BLOCK_SIZE: u32 = 0x40000;
/// Size of the fixed header, in bytes.
pub const HEADER_SIZE: usize = 24;

/// High index bit: this block is stored uncompressed.
pub const INDEX_UNCOMPRESSED: u32 = 0x8000_0000;
/// Mask for the offset half of an index entry.
pub const INDEX_OFFSET_MASK: u32 = 0x7FFF_FFFF;

pub const CSO_MAGIC: &[u8; 4] = b"CISO";
pub const ZSO_MAGIC: &[u8; 4] = b"ZISO";

/// Which flavour of the container we are reading or writing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// CSO v1: compressed blocks hold raw DEFLATE.
    Cso,
    /// ZSO: compressed blocks hold an LZ4 block.
    Zso,
}

impl Format {
    pub fn magic(self) -> &'static [u8; 4] {
        match self {
            Format::Cso => CSO_MAGIC,
            Format::Zso => ZSO_MAGIC,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Cso => "cso",
            Format::Zso => "zso",
        }
    }

    /// The codec a compressed block uses in this container.
    pub fn codec(self) -> Codec {
        match self {
            Format::Cso => Codec::Deflate,
            Format::Zso => Codec::Lz4,
        }
    }
}

/// The codec carried by a compressed block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Codec {
    Deflate,
    Lz4,
}

/// A parsed CSO/ZSO header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub format: Format,
    pub uncompressed_size: u64,
    pub block_size: u32,
    pub index_shift: u32,
}

impl Header {
    /// Parse and validate a 24-byte header.
    pub fn parse(bytes: &[u8]) -> Result<Header> {
        if bytes.len() < HEADER_SIZE {
            return Err(Error::Corrupt("file is shorter than a CSO header".into()));
        }
        let tag = &bytes[0..4];
        let format = if tag == CSO_MAGIC {
            Format::Cso
        } else if tag == ZSO_MAGIC {
            Format::Zso
        } else {
            return Err(Error::BadMagic {
                expected: "CISO or ZISO",
                found: escape_tag(tag),
            });
        };

        let uncompressed_size = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
        let block_size = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
        let version = bytes[20];
        let index_shift = bytes[21] as u32;

        // CSO v2 borrows this header but redefines the index, and ZSO v2 would be
        // meaningless; neither is supported.
        if version > 1 {
            return Err(Error::Unsupported(format!(
                "{format:?} version {version} (only version 0/1 is supported)"
            )));
        }
        if !(SECTOR_SIZE..=MAX_BLOCK_SIZE).contains(&block_size) {
            return Err(Error::Corrupt(format!(
                "block size {block_size} is outside {SECTOR_SIZE}..={MAX_BLOCK_SIZE}"
            )));
        }
        if !block_size.is_power_of_two() {
            return Err(Error::Corrupt(format!(
                "block size {block_size} is not a power of two"
            )));
        }
        if uncompressed_size % u64::from(SECTOR_SIZE) != 0 {
            return Err(Error::Corrupt(
                "uncompressed size is not aligned to the sector size".into(),
            ));
        }
        if index_shift > 32 {
            return Err(Error::Corrupt(format!("index shift {index_shift} is out of range")));
        }

        Ok(Header {
            format,
            uncompressed_size,
            block_size,
            index_shift,
        })
    }

    /// Serialize into a 24-byte buffer, little endian.
    pub fn write(self, out: &mut [u8; HEADER_SIZE]) {
        out[0..4].copy_from_slice(self.format.magic());
        out[4..8].copy_from_slice(&(HEADER_SIZE as u32).to_le_bytes());
        out[8..16].copy_from_slice(&self.uncompressed_size.to_le_bytes());
        out[16..20].copy_from_slice(&self.block_size.to_le_bytes());
        out[20] = 1;
        out[21] = self.index_shift as u8;
        out[22] = 0;
        out[23] = 0;
    }

    /// Number of blocks the payload spans.
    pub fn block_count(&self) -> u64 {
        block_count(self.uncompressed_size, self.block_size)
    }

    /// Byte length of the index, including the terminating entry.
    pub fn index_len(&self) -> u64 {
        (self.block_count() + 1) * 4
    }

    /// Offset where block data would start before alignment.
    pub fn data_start_unaligned(&self) -> u64 {
        HEADER_SIZE as u64 + self.index_len()
    }
}

pub fn block_count(size: u64, block_size: u32) -> u64 {
    size.div_ceil(u64::from(block_size))
}

/// Round `v` up to the next multiple of `a` (a power of two).
pub fn align_up(v: u64, a: u64) -> u64 {
    debug_assert!(a.is_power_of_two());
    (v + a - 1) & !(a - 1)
}

/// Pick the smallest shift that keeps every byte offset representable in the
/// 31 low bits of an index entry.
///
/// `worst_case_size` is the largest possible output size: header plus index plus
/// the whole payload uncompressed.
pub fn compute_index_shift(worst_case_size: u64) -> u32 {
    let bits = 64 - worst_case_size.leading_zeros();
    bits.saturating_sub(31)
}

fn escape_tag(tag: &[u8]) -> String {
    tag.iter()
        .map(|b| {
            if b.is_ascii_graphic() || *b == b' ' {
                (*b as char).to_string()
            } else {
                format!("\\x{b:02x}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_is_zero_below_2gb() {
        assert_eq!(compute_index_shift(1 << 30), 0);
        assert_eq!(compute_index_shift((1 << 31) - 1), 0);
    }

    #[test]
    fn shift_grows_one_bit_per_gigabyte() {
        assert_eq!(compute_index_shift(1 << 31), 1);
        assert_eq!(compute_index_shift(1 << 32), 2);
        assert_eq!(compute_index_shift(1 << 40), 10);
        assert_eq!(compute_index_shift(1 << 62), 32);
    }

    #[test]
    fn shifted_worst_case_fits_31_bits() {
        for size in [1u64 << 31, 1 << 35, 1 << 44, 1 << 62] {
            let shift = compute_index_shift(size);
            assert!(
                size >> shift <= INDEX_OFFSET_MASK as u64,
                "size {size} >> {shift} overflows 31 bits"
            );
        }
    }

    #[test]
    fn header_roundtrips() {
        let h = Header {
            format: Format::Zso,
            uncompressed_size: 1_825_361_920, // 891_290 sectors
            block_size: 2048,
            index_shift: 0,
        };
        let mut buf = [0u8; HEADER_SIZE];
        h.write(&mut buf);
        assert_eq!(Header::parse(&buf).unwrap(), h);
    }

    #[test]
    fn rejects_unknown_magic() {
        let mut buf = [0u8; HEADER_SIZE];
        buf[0..4].copy_from_slice(b"ABCD");
        assert!(matches!(
            Header::parse(&buf),
            Err(Error::BadMagic { .. })
        ));
    }

    #[test]
    fn rejects_cso_v2() {
        let h = Header {
            format: Format::Cso,
            uncompressed_size: 4096,
            block_size: 2048,
            index_shift: 0,
        };
        let mut buf = [0u8; HEADER_SIZE];
        h.write(&mut buf);
        buf[20] = 2;
        assert!(matches!(
            Header::parse(&buf),
            Err(Error::Unsupported(_))
        ));
    }

    #[test]
    fn rejects_unaligned_and_bad_block_size() {
        let h = Header {
            format: Format::Cso,
            uncompressed_size: 4097,
            block_size: 2048,
            index_shift: 0,
        };
        let mut buf = [0u8; HEADER_SIZE];
        h.write(&mut buf);
        assert!(matches!(Header::parse(&buf), Err(Error::Corrupt(_))));

        let h = Header {
            format: Format::Cso,
            uncompressed_size: 4096,
            block_size: 3000,
            index_shift: 0,
        };
        h.write(&mut buf);
        assert!(matches!(Header::parse(&buf), Err(Error::Corrupt(_))));
    }

    #[test]
    fn align_up_is_a_no_op_when_already_aligned() {
        assert_eq!(align_up(4096, 2048), 4096);
        assert_eq!(align_up(4097, 2048), 6144);
        assert_eq!(align_up(0, 1), 0);
    }
}

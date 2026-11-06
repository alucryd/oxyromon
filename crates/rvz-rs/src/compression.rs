//! The compression methods RVZ allows, each set up as Dolphin sets it up, so
//! that what it writes is what Dolphin writes wherever the codec allows: zstd
//! and bzip2 byte for byte, LZMA readable either way.

use std::io::{Read, Write};
use std::ops::RangeInclusive;

use lzma_rust2::{Lzma2Options, Lzma2Reader, LzmaOptions, LzmaReader, LzmaWriter};

use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compression {
    None,
    Bzip2,
    Lzma,
    Lzma2,
    Zstd,
}

impl Compression {
    /// How the header names it.
    pub fn id(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Bzip2 => 2,
            Self::Lzma => 3,
            Self::Lzma2 => 4,
            Self::Zstd => 5,
        }
    }

    /// The method a header names, `None` for one RVZ does not allow, such as
    /// WIA's purge.
    pub fn from_id(id: u32) -> Option<Self> {
        match id {
            0 => Some(Self::None),
            2 => Some(Self::Bzip2),
            3 => Some(Self::Lzma),
            4 => Some(Self::Lzma2),
            5 => Some(Self::Zstd),
            _ => None,
        }
    }

    /// The levels it takes. Zstandard's negative levels are left out, as
    /// Dolphin's own converter leaves them out.
    pub fn levels(self) -> RangeInclusive<i32> {
        match self {
            Self::None => 0..=0,
            Self::Bzip2 | Self::Lzma | Self::Lzma2 => 1..=9,
            Self::Zstd => 1..=zstd::compression_level_range().end().to_owned(),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Bzip2 => "bzip2",
            Self::Lzma => "lzma",
            Self::Lzma2 => "lzma2",
            Self::Zstd => "zstd",
        }
    }
}

/// LZMA2's dictionary size for a header's encoded value.
fn lzma2_dictionary_size(value: u8) -> u32 {
    (2 | (value as u32 & 1)) << (value / 2 + 11)
}

/// A compression method at a level.
#[derive(Clone, Copy, Debug)]
pub struct Compressor {
    pub compression: Compression,
    pub level: i32,
}

impl Compressor {
    pub fn new(compression: Compression, level: i32) -> Result<Self> {
        if compression != Compression::None && !compression.levels().contains(&level) {
            return Err(Error::InvalidOption(format!(
                "{} takes a level in {}..={}, not {level}",
                compression.name(),
                compression.levels().start(),
                compression.levels().end()
            )));
        }
        // As Dolphin's converter does, no level for no compression
        let level = if compression == Compression::None {
            0
        } else {
            level
        };
        Ok(Self { compression, level })
    }

    /// What the header keeps for the decompressor: its size, and the bytes.
    pub fn header_data(&self) -> (u8, [u8; 7]) {
        let mut data = [0; 7];
        match self.compression {
            Compression::Lzma => {
                let options = LzmaOptions::with_preset(self.level as u32);
                data[0] = options.get_props();
                data[1..5].copy_from_slice(&options.dict_size.to_le_bytes());
                (5, data)
            }
            Compression::Lzma2 => {
                let dict_size = LzmaOptions::with_preset(self.level as u32).dict_size;
                let mut value = 0;
                while value < 40 && dict_size > lzma2_dictionary_size(value) {
                    value += 1;
                }
                data[0] = value;
                (1, data)
            }
            _ => (0, data),
        }
    }

    /// Compress `parts` as one stream.
    pub fn compress(&self, parts: &[&[u8]]) -> Result<Vec<u8>> {
        let total: usize = parts.iter().map(|part| part.len()).sum();
        let mut out = Vec::with_capacity(total / 2);
        match self.compression {
            Compression::None => parts.iter().for_each(|part| out.extend_from_slice(part)),
            Compression::Bzip2 => {
                let mut encoder =
                    bzip2::write::BzEncoder::new(out, bzip2::Compression::new(self.level as u32));
                parts.iter().try_for_each(|part| encoder.write_all(part))?;
                out = encoder.finish()?;
            }
            Compression::Lzma => {
                let options = LzmaOptions::with_preset(self.level as u32);
                let mut writer = LzmaWriter::new_no_header(out, &options, true)?;
                parts.iter().try_for_each(|part| writer.write_all(part))?;
                out = writer.finish()?;
            }
            Compression::Lzma2 => {
                let options = Lzma2Options::with_preset(self.level as u32);
                let mut writer = lzma_rust2::Lzma2Writer::new(out, options);
                parts.iter().try_for_each(|part| writer.write_all(part))?;
                out = writer.finish()?;
            }
            Compression::Zstd => {
                let mut encoder = zstd::stream::write::Encoder::new(out, self.level)?;
                encoder.include_contentsize(false)?;
                encoder.set_pledged_src_size(Some(total as u64))?;
                parts.iter().try_for_each(|part| encoder.write_all(part))?;
                out = encoder.finish()?;
            }
        }
        Ok(out)
    }
}

/// Decompress a whole stream, `data` holding what the header keeps for the
/// decompressor, failing rather than expanding it past `max` bytes, so that a
/// crafted file cannot exhaust memory.
pub fn decompress(
    compression: Compression,
    data: &[u8],
    input: &[u8],
    max: usize,
) -> Result<Vec<u8>> {
    let corrupt =
        |error: std::io::Error| Error::Corrupt(format!("{} stream: {error}", compression.name()));
    let mut out = Vec::new();
    let reader: Box<dyn Read + '_> = match compression {
        Compression::None => Box::new(input),
        Compression::Bzip2 => Box::new(bzip2::read::BzDecoder::new(input)),
        Compression::Lzma => {
            if data.len() < 5 || data[0] >= 9 * 5 * 5 {
                return Err(Error::Corrupt("bad LZMA properties".into()));
            }
            let dict_size = u32::from_le_bytes(data[1..5].try_into().unwrap());
            Box::new(
                LzmaReader::new_with_props(input, u64::MAX, data[0], dict_size, None)
                    .map_err(corrupt)?,
            )
        }
        Compression::Lzma2 => {
            if data.is_empty() || data[0] > 40 {
                return Err(Error::Corrupt("bad LZMA2 properties".into()));
            }
            let dict_size = if data[0] == 40 {
                u32::MAX
            } else {
                lzma2_dictionary_size(data[0])
            };
            Box::new(Lzma2Reader::new(input, dict_size, None))
        }
        Compression::Zstd => Box::new(zstd::stream::read::Decoder::new(input).map_err(corrupt)?),
    };
    reader
        .take(max as u64 + 1)
        .read_to_end(&mut out)
        .map_err(corrupt)?;
    if out.len() > max {
        return Err(Error::Corrupt(format!(
            "{} stream expands past {max} bytes",
            compression.name()
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_method_round_trips() {
        let data: Vec<u8> = (0..100_000u32).map(|i| (i / 7 % 251) as u8).collect();
        for compression in [
            Compression::None,
            Compression::Bzip2,
            Compression::Lzma,
            Compression::Lzma2,
            Compression::Zstd,
        ] {
            let compressor = Compressor::new(
                compression,
                if compression == Compression::None {
                    0
                } else {
                    5
                },
            )
            .unwrap();
            let packed = compressor.compress(&[&data[..10], &data[10..]]).unwrap();
            let (size, header) = compressor.header_data();
            let header = &header[..size as usize];
            let unpacked = decompress(compression, header, &packed, data.len()).unwrap();
            assert_eq!(unpacked, data, "{compression:?}");
            // One byte short of what it expands to is a corrupt stream
            assert!(matches!(
                decompress(compression, header, &packed, data.len() - 1),
                Err(Error::Corrupt(_))
            ));
        }
    }

    #[test]
    fn lzma2_dictionary_sizes_encode_as_dolphin_encodes_them() {
        let compressor = Compressor::new(Compression::Lzma2, 9).unwrap();
        // 64 MiB is 2 << 25
        assert_eq!(compressor.header_data().0, 1);
        assert_eq!(
            lzma2_dictionary_size(compressor.header_data().1[0]),
            64 << 20
        );
    }
}

//! Compressing a group, with each codec's parameters pinned.

use std::io::{Read, Write};

use lzma_rust2::{Lzma2Options, Lzma2Reader, Lzma2Writer};
use zstd::zstd_safe::CParameter;

use super::error::{Error, Result};
use super::format::Codec;

pub const ZSTD_LEVEL: i32 = 19;
pub const LZMA2_PRESET: u32 = 9;

/// The window or dictionary for a group of at most `group` bytes.
fn window_log(group: usize) -> u32 {
    group.next_power_of_two().max(4096).trailing_zeros()
}

pub fn compress(codec: Codec, data: &[u8], group: usize) -> Result<Vec<u8>> {
    match codec {
        Codec::Zstd => {
            let mut compressor = zstd::bulk::Compressor::new(ZSTD_LEVEL)?;
            compressor.set_parameter(CParameter::WindowLog(window_log(group)))?;
            compressor.set_parameter(CParameter::ChecksumFlag(false))?;
            Ok(compressor.compress(data)?)
        }
        Codec::Lzma2 => {
            let mut options = Lzma2Options::with_preset(LZMA2_PRESET);
            options.lzma_options.dict_size = 1 << window_log(group);
            let mut writer = Lzma2Writer::new(Vec::new(), options);
            writer.write_all(data)?;
            Ok(writer.finish()?)
        }
    }
}

pub fn decompress(codec: Codec, data: &[u8], length: usize) -> Result<Vec<u8>> {
    let out = match codec {
        Codec::Zstd => zstd::bulk::decompress(data, length)?,
        Codec::Lzma2 => {
            let mut out = Vec::with_capacity(length);
            Lzma2Reader::new(data, 1 << window_log(length), None)
                .take(length as u64 + 1)
                .read_to_end(&mut out)?;
            out
        }
    };
    if out.len() != length {
        return Err(Error::Corrupt(format!(
            "a group decoded to {} bytes, not {length}",
            out.len()
        )));
    }
    Ok(out)
}

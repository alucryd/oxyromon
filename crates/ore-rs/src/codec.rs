//! Compressing a group, with each codec's parameters pinned.

use std::io::{Read, Write};

use lzma_rust2::{Lzma2Options, Lzma2Reader, Lzma2Writer};
use zstd::zstd_safe::CParameter;

use rusty_flac::{Encoder, FrameDecoder, StreamInfo};

use super::error::{Error, Result};
use super::format::Codec;

pub const ZSTD_LEVEL: i32 = 19;
pub const LZMA2_PRESET: u32 = 9;
pub const FLAC_LEVEL: u32 = 8;
/// 8 CD sectors of samples: as good as FLAC's default 4,096, and on
/// sector boundaries.
pub const FLAC_BLOCK: u32 = 4704;
/// The `fLaC` marker and its STREAMINFO, which every group would repeat:
/// a group keeps its frames alone.
const FLAC_CONTAINER: usize = 8 + 34;

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
        Codec::Flac => flac_encode(data),
    }
}

fn flac_info() -> StreamInfo {
    StreamInfo {
        min_block_size: FLAC_BLOCK,
        max_block_size: FLAC_BLOCK,
        sample_rate: 44_100,
        channels: 2,
        bits_per_sample: 16,
        ..StreamInfo::default()
    }
}

/// CD PCM, 16-bit little-endian stereo, as FLAC frames.
fn flac_encode(data: &[u8]) -> Result<Vec<u8>> {
    if !data.len().is_multiple_of(4) {
        return Err(Error::InvalidOption(
            "FLAC takes whole stereo samples".into(),
        ));
    }
    let failed = |e: rusty_flac::EncodeError| Error::Corrupt(format!("FLAC: {e}"));
    let mut encoder = Encoder::new(44_100, 2, 16).map_err(failed)?;
    encoder.set_compression_level(FLAC_LEVEL);
    encoder.set_block_size(FLAC_BLOCK).map_err(failed)?;
    let samples: Vec<i32> = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&sample| i16::from_le_bytes(sample) as i32)
        .collect();
    encoder.push_interleaved(&samples).map_err(failed)?;
    let mut stream = encoder.finish();
    if stream.len() < FLAC_CONTAINER || !stream.starts_with(b"fLaC") {
        return Err(Error::Corrupt("FLAC: an unexpected stream".into()));
    }
    stream.drain(..FLAC_CONTAINER);
    Ok(stream)
}

fn flac_decode(data: &[u8], length: usize) -> Result<Vec<u8>> {
    let corrupt = |e| Error::Corrupt(format!("FLAC: {e}"));
    let mut decoder = FrameDecoder::new(flac_info()).map_err(corrupt)?;
    let mut samples: Vec<i16> = Vec::with_capacity(length / 2);
    let mut at = 0;
    while samples.len() < length / 2 && at < data.len() {
        let (_, used) = decoder
            .decode_frame_interleaved(&data[at..], &mut samples)
            .map_err(corrupt)?;
        if used == 0 {
            break;
        }
        at += used;
    }
    Ok(samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect())
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
        Codec::Flac => flac_decode(data, length)?,
    };
    if out.len() != length {
        return Err(Error::Corrupt(format!(
            "a group decoded to {} bytes, not {length}",
            out.len()
        )));
    }
    Ok(out)
}

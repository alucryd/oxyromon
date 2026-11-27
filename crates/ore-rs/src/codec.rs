// SPDX-License-Identifier: MIT OR Apache-2.0
//! Compressing a group, with each codec's parameters pinned.

use std::io::{Read, Write};

use lzma_rust2::{Lzma2Options, Lzma2Reader, Lzma2Writer};
use zstd::zstd_safe::CParameter;

use rusty_flac::{Encoder, FrameDecoder, StreamInfo};

use super::error::{Error, Result};
use super::format::Codec;
use super::vag;
use super::video;
use super::xa;

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

/// The zstd level that tries whether a group meant for `xa` or `str` is
/// what they model at all.
const PROBE_LEVEL: i32 = 3;

/// Compress a group with `codec`, or, for a codec of one kind of data,
/// with Zstandard when it does better: some discs flag data as XA audio, a
/// header can lie, and some CDs keep data in audio tracks. A quick try of
/// Zstandard picks the groups worth its full level, which codes about a
/// quarter smaller. A group no quick try of Zstandard can shrink is that
/// try's, whatever the codec.
/// Returns the codec used.
pub fn compress_best(codec: Codec, data: &[u8], group: usize) -> Result<(Codec, Vec<u8>)> {
    // Noise, as an Xbox disc's filler is, codes no smaller at any level: a
    // quick try that cannot shrink it at all is kept, sparing the full one
    if matches!(codec, Codec::Zstd | Codec::Lzma2) {
        // with the full level's window, so no match it would find is missed
        let mut compressor = zstd::bulk::Compressor::new(PROBE_LEVEL)?;
        compressor.set_parameter(CParameter::WindowLog(window_log(group)))?;
        let probe = compressor.compress(data)?;
        if probe.len() >= data.len() {
            return Ok((Codec::Zstd, probe));
        }
    }
    let stored = compress(codec, data, group)?;
    if matches!(
        codec,
        Codec::Flac | Codec::Xa | Codec::Str | Codec::Vag | Codec::Pcm
    ) && zstd::bulk::compress(data, PROBE_LEVEL)?.len() < stored.len() + stored.len() / 4
    {
        let zstd = compress(Codec::Zstd, data, group)?;
        if zstd.len() < stored.len() {
            return Ok((Codec::Zstd, zstd));
        }
    }
    Ok((codec, stored))
}

/// Compress a group with `codec`, to no less than the 1,024th of it a
/// reader refuses below as a decompression bomb: a still picture, silence
/// or a repeated pattern codes to less, and is padded with what its
/// decoder ignores, a Zstandard skippable frame, or zeros after the end.
pub fn compress(codec: Codec, data: &[u8], group: usize) -> Result<Vec<u8>> {
    let mut stored = compress_unpadded(codec, data, group)?;
    let floor = data.len().div_ceil(1024);
    if stored.len() < floor {
        if codec == Codec::Zstd {
            let size = floor.saturating_sub(stored.len() + 8);
            stored.extend_from_slice(&0x184D_2A50u32.to_le_bytes());
            stored.extend_from_slice(&(size as u32).to_le_bytes());
            stored.resize(stored.len() + size, 0);
        } else {
            stored.resize(floor, 0);
        }
    }
    Ok(stored)
}

fn compress_unpadded(codec: Codec, data: &[u8], group: usize) -> Result<Vec<u8>> {
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
        Codec::Xa => Ok(xa::encode(data)),
        Codec::Str => Ok(video::encode(data)),
        Codec::Vag => Ok(vag::encode(data)),
        Codec::Pcm => pcm_encode(data),
    }
}

fn flac_info(channels: u32) -> StreamInfo {
    StreamInfo {
        min_block_size: FLAC_BLOCK,
        max_block_size: FLAC_BLOCK,
        sample_rate: 44_100,
        channels,
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
    flac_frames(&samples(data), 2)
}

fn samples(data: &[u8]) -> Vec<i32> {
    data.as_chunks::<2>()
        .0
        .iter()
        .map(|&sample| i16::from_le_bytes(sample) as i32)
        .collect()
}

/// Samples, `channels` of them interleaved, as FLAC frames alone.
fn flac_frames(samples: &[i32], channels: u32) -> Result<Vec<u8>> {
    if samples.is_empty() {
        return Ok(Vec::new());
    }
    let failed = |e: rusty_flac::EncodeError| Error::Corrupt(format!("FLAC: {e}"));
    let mut encoder = Encoder::new(44_100, channels, 16).map_err(failed)?;
    encoder.set_compression_level(FLAC_LEVEL);
    encoder.set_block_size(FLAC_BLOCK).map_err(failed)?;
    encoder.push_interleaved(samples).map_err(failed)?;
    let mut stream = encoder.finish();
    if stream.len() < FLAC_CONTAINER || !stream.starts_with(b"fLaC") {
        return Err(Error::Corrupt("FLAC: an unexpected stream".into()));
    }
    stream.drain(..FLAC_CONTAINER);
    Ok(stream)
}

/// `count` samples of `channels` interleaved out of FLAC frames, and the
/// bytes of frames they took.
fn flac_samples(data: &[u8], channels: u32, count: usize) -> Result<(Vec<i16>, usize)> {
    let corrupt = |e| Error::Corrupt(format!("FLAC: {e}"));
    let mut decoder = FrameDecoder::new(flac_info(channels)).map_err(corrupt)?;
    let mut samples: Vec<i16> = Vec::with_capacity(count);
    let mut at = 0;
    while samples.len() < count && at < data.len() {
        let (_, used) = decoder
            .decode_frame_interleaved(&data[at..], &mut samples)
            .map_err(corrupt)?;
        if used == 0 {
            break;
        }
        at += used;
    }
    Ok((samples, at))
}

fn flac_decode(data: &[u8], length: usize) -> Result<Vec<u8>> {
    let (samples, _) = flac_samples(data, 2, length / 2)?;
    Ok(samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect())
}

/// How a `pcm` group's 16-bit little-endian samples lie, by its first
/// byte: their channels, and the bytes of each channel before the next's.
const PCM_LAYOUTS: [(u32, usize); 3] = [(1, 2), (2, 2), (2, 512)];
/// How much of a group picks its layout.
const PCM_PROBE: usize = 256 << 10;

/// A layout's samples, interleaved a sample at a time, and the bytes past
/// its last whole period, which are left as they are.
fn pcm_samples(data: &[u8], (channels, interleave): (u32, usize)) -> (Vec<i32>, usize) {
    let period = channels as usize * interleave;
    let whole = data.len() / period * period;
    let samples = samples(&data[..whole]);
    if interleave == 2 {
        return (samples, whole);
    }
    let per = interleave / 2;
    let mut out = Vec::with_capacity(samples.len());
    for block in samples.chunks(per * channels as usize) {
        for i in 0..per {
            out.extend((0..channels as usize).map(|c| block[c * per + i]));
        }
    }
    (out, whole)
}

/// 16-bit PCM, as its layout, its samples' FLAC frames, then the bytes
/// past them: the layout whose FLAC is smallest on the group's start.
fn pcm_encode(data: &[u8]) -> Result<Vec<u8>> {
    let probe = &data[..data.len().min(PCM_PROBE)];
    let mut best = (usize::MAX, 0);
    for (i, &layout) in PCM_LAYOUTS.iter().enumerate() {
        let (samples, whole) = pcm_samples(probe, layout);
        let size = flac_frames(&samples, layout.0)?.len() + probe.len() - whole;
        best = best.min((size, i));
    }
    let layout = PCM_LAYOUTS[best.1];
    let (samples, whole) = pcm_samples(data, layout);
    let mut out = vec![best.1 as u8];
    out.extend(flac_frames(&samples, layout.0)?);
    out.extend_from_slice(&data[whole..]);
    Ok(out)
}

fn pcm_decode(data: &[u8], length: usize) -> Result<Vec<u8>> {
    let corrupt = || Error::Corrupt("a pcm group's layout".into());
    let &(channels, interleave) = PCM_LAYOUTS
        .get(*data.first().ok_or_else(corrupt)? as usize)
        .ok_or_else(corrupt)?;
    let period = channels as usize * interleave;
    let whole = length / period * period;
    let (samples, used) = flac_samples(&data[1..], channels, whole / 2)?;
    if samples.len() != whole / 2 {
        return Err(Error::Corrupt("a pcm group's samples cut short".into()));
    }
    let mut out = Vec::with_capacity(length);
    let per = interleave / 2;
    for block in samples.chunks(per * channels as usize) {
        for c in 0..channels as usize {
            for i in 0..per {
                out.extend_from_slice(&block[i * channels as usize + c].to_le_bytes());
            }
        }
    }
    // Past the tail, what padding the group took to its floor
    let tail = data[1 + used..]
        .get(..length - whole)
        .ok_or_else(|| Error::Corrupt("a pcm group's tail cut short".into()))?;
    out.extend_from_slice(tail);
    Ok(out)
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
        Codec::Xa => xa::decode(data, length),
        Codec::Str => video::decode(data, length),
        Codec::Vag => vag::decode(data, length),
        Codec::Pcm => pcm_decode(data, length)?,
    };
    if out.len() != length {
        return Err(Error::Corrupt(format!(
            "a group decoded to {} bytes, not {length}",
            out.len()
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_repeated_past_a_quick_tries_window_is_not_kept_as_noise() {
        let mut state = 7u64;
        let mut data: Vec<u8> = (0..5 << 19)
            .map(|_| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                (state >> 56) as u8
            })
            .collect();
        // The repeat lies 2.5 MiB back, past level 3's own 2 MiB window
        data.extend_from_within(..3 << 19);
        let (codec, stored) = compress_best(Codec::Zstd, &data, 4 << 20).unwrap();
        assert!(stored.len() < data.len() * 3 / 4, "{}", stored.len());
        assert_eq!(decompress(codec, &stored, data.len()).unwrap(), data);
    }

    #[test]
    fn data_in_an_audio_group_takes_zstd() {
        let data: Vec<u8> = b"ATARI APPROVED DATA HEADER ATRI "
            .iter()
            .cycle()
            .take(64 * 2352)
            .copied()
            .collect();
        let (codec, stored) = compress_best(Codec::Flac, &data, data.len()).unwrap();
        assert_eq!(codec, Codec::Zstd);
        assert_eq!(decompress(codec, &stored, data.len()).unwrap(), data);
    }

    #[test]
    fn every_codec_keeps_to_the_bomb_ratio_and_round_trips() {
        // A pattern every codec codes to a few bytes a MiB
        let data: Vec<u8> = (0..1 << 20).map(|i| (i % 4 * 64) as u8).collect();
        for codec in [
            Codec::Zstd,
            Codec::Lzma2,
            Codec::Flac,
            Codec::Xa,
            Codec::Str,
            Codec::Vag,
            Codec::Pcm,
        ] {
            // xa and str code it a byte at a time: tests/cd.rs's still
            // picture takes str under
            let unpadded = compress_unpadded(codec, &data, data.len()).unwrap();
            assert!(
                matches!(codec, Codec::Xa | Codec::Str | Codec::Vag)
                    || unpadded.len() < data.len() / 1024,
                "{codec:?} pads"
            );
            let stored = compress(codec, &data, data.len()).unwrap();
            assert!(
                stored.len() >= data.len() / 1024,
                "{codec:?}: {}",
                stored.len()
            );
            assert!(
                decompress(codec, &stored, data.len()).unwrap() == data,
                "{codec:?}"
            );
        }
    }
}

//! FLAC, as used by the `flac`, `cdfl` and `avhu` CHD codecs, through
//! rusty_flac.
//!
//! Unlike a `.flac` file, a CHD chunk carries no FLAC container: no `fLaC`
//! magic and no STREAMINFO block, only the frames. Like MAME, we decode them
//! against the fixed STREAMINFO the compressors wrote their stream for, and
//! strip the container the encoder writes in front of them.

use rusty_flac::EncodeError;
use rusty_flac::Encoder;
use rusty_flac::FrameDecoder;
use rusty_flac::StreamInfo;

use crate::error::Error;
use crate::error::Result;

/// The stream format of the `flac` and `cdfl` codecs: CD audio.
const CD_CHANNELS: u32 = 2;
const CD_SAMPLE_RATE: u32 = 44_100;

/// The container rusty_flac writes before the frames: the `fLaC` magic, then
/// its one metadata block, STREAMINFO, as a 4-byte header (last block, type 0,
/// 34 bytes long) and its 34-byte body.
const CONTAINER_HEADER: &[u8; 8] = b"fLaC\x80\x00\x00\x22";
const CONTAINER_SIZE: usize = CONTAINER_HEADER.len() + 34;

/// Decodes a headerless FLAC `payload` of 44.1 kHz stereo into `out`, where
/// `block_size` is the block size the encoder used and the samples are
/// written big-endian when `big_endian` says so. Returns the number of
/// payload bytes the FLAC stream consumed; the rest of the payload, if any,
/// is another codec's.
pub(crate) fn flac_decode(
    payload: &[u8],
    out: &mut [u8],
    block_size: u32,
    big_endian: bool,
) -> Result<usize> {
    flac_decode_with(
        payload,
        out,
        CD_CHANNELS,
        CD_SAMPLE_RATE,
        block_size,
        big_endian,
    )
}

/// [`flac_decode`] for a stream of any channel count and sample rate, the
/// samples of `out` interleaved.
pub(crate) fn flac_decode_with(
    payload: &[u8],
    out: &mut [u8],
    channels: u32,
    sample_rate: u32,
    block_size: u32,
    big_endian: bool,
) -> Result<usize> {
    let width = channels as usize * 2;
    if channels == 0 || !out.len().is_multiple_of(width) {
        return Err(Error::Corrupt(
            "FLAC chunk output is not a whole number of samples".to_string(),
        ));
    }
    let corrupt = |error| Error::Corrupt(format!("corrupt FLAC stream: {error}"));
    // The STREAMINFO MAME synthesizes for a headerless chunk: 16-bit
    // samples at a fixed block size, with no frame sizes, sample count or
    // md5 sum.
    let info = StreamInfo {
        min_block_size: block_size,
        max_block_size: block_size,
        sample_rate,
        channels,
        bits_per_sample: 16,
        ..StreamInfo::default()
    };
    let mut decoder = FrameDecoder::new(info).map_err(corrupt)?;
    let target = out.len() / 2;
    let mut samples: Vec<i16> = Vec::with_capacity(target);
    let mut position = 0;
    while samples.len() < target {
        let (_, used) = decoder
            .decode_frame_interleaved(&payload[position..], &mut samples)
            .map_err(corrupt)?;
        position += used;
    }
    // A last frame running past the output is cut short, as MAME does.
    for (bytes, sample) in out.as_chunks_mut::<2>().0.iter_mut().zip(&samples) {
        *bytes = if big_endian {
            sample.to_be_bytes()
        } else {
            sample.to_le_bytes()
        };
    }
    Ok(position)
}

/// Encodes `plane` as 16-bit 44.1 kHz stereo samples in a headerless FLAC
/// stream at `block_size`, the mirror of MAME's FLAC compressor: the frames
/// the encoder produces, with the magic and STREAMINFO stripped.
/// `big_endian` says how the sample pairs in the plane are byte ordered,
/// which is how the plain FLAC codec tries both readings of a hunk.
pub(crate) fn flac_encode(plane: &[u8], block_size: u32, big_endian: bool) -> Result<Vec<u8>> {
    flac_encode_with(plane, CD_CHANNELS, CD_SAMPLE_RATE, block_size, big_endian)
}

/// [`flac_encode`] for interleaved samples of any channel count and sample
/// rate.
pub(crate) fn flac_encode_with(
    plane: &[u8],
    channels: u32,
    sample_rate: u32,
    block_size: u32,
    big_endian: bool,
) -> Result<Vec<u8>> {
    let width = channels as usize * 2;
    if channels == 0 || !plane.len().is_multiple_of(width) {
        return Err(Error::Compression(
            "FLAC chunk input is not a whole number of samples".to_string(),
        ));
    }
    let compression = |error: EncodeError| {
        Error::Compression(format!("cannot compress the FLAC stream: {error}"))
    };
    let mut encoder = Encoder::new(sample_rate, channels, 16).map_err(compression)?;
    // MAME's level, the highest
    encoder.set_compression_level(8);
    encoder.set_block_size(block_size).map_err(compression)?;
    let samples: Vec<i32> = plane
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&value| {
            i32::from(if big_endian {
                i16::from_be_bytes(value)
            } else {
                i16::from_le_bytes(value)
            })
        })
        .collect();
    encoder.push_interleaved(&samples).map_err(compression)?;
    let mut stream = encoder.finish();
    // the frames are cut at a fixed offset, so a container of another shape
    // would misalign every hunk: refuse it in release builds too
    if stream.len() < CONTAINER_SIZE || !stream.starts_with(CONTAINER_HEADER) {
        return Err(Error::Compression(
            "unexpected FLAC container: not fLaC and a single STREAMINFO".to_string(),
        ));
    }
    stream.drain(..CONTAINER_SIZE);
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flac_encode_roundtrips_through_flac_decode() {
        // A sector plane of eight CD frames of noise, and the block size
        // the cdfl codec derives from a 19584-byte hunk.
        let mut plane = Vec::new();
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        while plane.len() < 8 * 2352 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            plane.extend_from_slice(&state.to_be_bytes());
        }
        plane.truncate(8 * 2352);
        let block_size = 2352;
        let encoded = flac_encode(&plane, block_size, true).unwrap();
        let mut decoded = vec![0; plane.len()];
        let consumed = flac_decode(&encoded, &mut decoded, block_size, true).unwrap();
        assert_eq!(consumed, encoded.len());
        assert_eq!(decoded, plane);
    }
}

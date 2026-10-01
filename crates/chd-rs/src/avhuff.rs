//! The `avhu` codec: LaserDisc frames, a port of MAME's
//! `src/lib/util/avhuff.cpp` as chdman 0.289 builds it.
//!
//! A hunk holds one video field and its audio in the raw form chdman
//! assembles, every multibyte value big-endian:
//!
//! ```text
//! +00 'chav'  +04 metadata size  +05 channels  +06 samples per channel
//! +08 width   +0A height         +0C metadata, then each channel's
//! samples, then the video as (Y, Cb/Cr) byte pairs
//! ```
//!
//! Compressed, the header shrinks to 8 bytes followed by the size of the
//! audio Huffman tables (0xffff: FLAC) and of each channel's stream; then
//! come the metadata, each channel as a headerless 48 kHz mono FLAC stream,
//! and the video as per-component deltas, run-length coded and Huffman
//! coded, a fresh table per component and field.

use crate::bitstream::{BitstreamIn, BitstreamOut};
use crate::error::{Error, Result};
use crate::flac::{flac_decode_with, flac_encode_with};
use crate::huffman::{HuffmanDecoder, HuffmanEncoder};

/// The sample rate the audio streams are FLAC-coded at, whatever the
/// LaserDisc's own.
const FLAC_SAMPLE_RATE: u32 = 48_000;
/// Delta symbols, then the run-length codes 0x100..=0x10f.
const DELTA_CODES: usize = 256 + 16;
/// MAME's default for `huffman_encoder` and `huffman_decoder`.
const MAX_BITS: u32 = 16;

fn be16(data: &[u8], offset: usize) -> u32 {
    u32::from(u16::from_be_bytes([data[offset], data[offset + 1]]))
}

/// The repetitions a run-length code stands for.
fn code_to_rlecount(code: u32) -> u32 {
    if code == 0x00 {
        1
    } else if code <= 0x107 {
        8 + (code - 0x100)
    } else {
        16 << (code - 0x108)
    }
}

/// The code for the longest run at most `rlecount` long; 0x00, a plain zero
/// delta, below eight.
fn rlecount_to_code(rlecount: u32) -> u32 {
    match rlecount {
        2048.. => 0x10f,
        1024.. => 0x10e,
        512.. => 0x10d,
        256.. => 0x10c,
        128.. => 0x10b,
        64.. => 0x10a,
        32.. => 0x109,
        16.. => 0x108,
        8.. => 0x100 + (rlecount - 8),
        _ => 0x00,
    }
}

/// The size of a raw frame, from its header; 0 for anything else.
pub(crate) fn raw_data_size(data: &[u8]) -> usize {
    if data.len() < 12 || &data[..4] != b"chav" {
        return 0;
    }
    12 + usize::from(data[4])
        + 2 * usize::from(data[5]) * be16(data, 6) as usize
        + 2 * be16(data, 8) as usize * (be16(data, 10) & 0x7fff) as usize
}

/// The size of a raw frame of these dimensions, without metadata.
pub(crate) fn raw_size(width: u32, height: u32, channels: u32, samples: u32) -> u32 {
    12 + channels * samples * 2 + width * height * 2
}

/// The delta and run-length coder of one video component.
struct DeltaRleEncoder {
    encoder: HuffmanEncoder,
    rle: Vec<u32>,
    cursor: usize,
    rlecount: u32,
}

impl DeltaRleEncoder {
    /// Run-length codes and histograms the component at every `advance`
    /// bytes of `source`, `items` a row, the delta carried across rows, then
    /// builds the tree: `rle_and_histo_bitmap`.
    fn new(source: &[u8], items: usize, advance: usize, rows: usize) -> Self {
        let mut encoder = HuffmanEncoder::new(DELTA_CODES, MAX_BITS);
        let mut rle = Vec::with_capacity(items * rows);
        let mut prevdata = 0u8;
        for row in 0..rows {
            let end = (row + 1) * items * advance;
            let mut position = row * items * advance;
            while position < end {
                let curdelta = source[position].wrapping_sub(prevdata);
                prevdata = source[position];
                if curdelta == 0 {
                    // a zero delta counts how many more follow in the row
                    let mut zerocount = 1u32;
                    let mut scan = position + advance;
                    while scan < end && source[scan] == prevdata {
                        zerocount += 1;
                        scan += advance;
                    }
                    // one reaching the end of the row may run on past it
                    if scan >= end && zerocount >= 8 {
                        zerocount = 100_000;
                    }
                    let code = rlecount_to_code(zerocount);
                    encoder.histo_one(code as usize);
                    rle.push(code);
                    position += (code_to_rlecount(code) - 1) as usize * advance;
                } else {
                    encoder.histo_one(usize::from(curdelta));
                    rle.push(u32::from(curdelta));
                }
                position += advance;
            }
        }
        // MAME ignores a failure here; exporting the tree reports it
        let _ = encoder.compute_tree_from_histo();
        Self {
            encoder,
            rle,
            cursor: 0,
            rlecount: 0,
        }
    }

    fn encode_one(&mut self, bitbuf: &mut BitstreamOut<'_>) {
        if self.rlecount != 0 {
            self.rlecount -= 1;
            return;
        }
        let data = self.rle[self.cursor];
        self.cursor += 1;
        self.encoder.encode_one(bitbuf, data as usize);
        if data >= 0x100 {
            self.rlecount = code_to_rlecount(data) - 1;
        }
    }
}

/// The delta and run-length decoder of one video component.
struct DeltaRleDecoder {
    decoder: HuffmanDecoder,
    prevdata: u8,
    rlecount: u32,
}

impl DeltaRleDecoder {
    fn new() -> Self {
        Self {
            decoder: HuffmanDecoder::new(DELTA_CODES, MAX_BITS),
            prevdata: 0,
            rlecount: 0,
        }
    }

    fn decode_one(&mut self, bitbuf: &mut BitstreamIn<'_>) -> u8 {
        if self.rlecount != 0 {
            self.rlecount -= 1;
            return self.prevdata;
        }
        let data = self.decoder.decode_one(bitbuf);
        if data < 0x100 {
            self.prevdata = self.prevdata.wrapping_add(data as u8);
        } else {
            self.rlecount = code_to_rlecount(data) - 1;
        }
        self.prevdata
    }
}

/// Compresses a raw frame the way `chd_avhuff_compressor::compress` does,
/// `source` being a whole hunk: whatever follows the frame must be zero,
/// and the result must not be larger than the hunk.
pub(crate) fn compress(source: &[u8]) -> Result<Vec<u8>> {
    let size = raw_data_size(source);
    if size == 0 || size > source.len() {
        return Err(Error::Corrupt("not an A/V frame".to_owned()));
    }
    if source[size..].iter().any(|&byte| byte != 0) {
        return Err(Error::Corrupt("A/V frame padding is not zero".to_owned()));
    }
    let out = encode_data(source)?;
    if out.len() > source.len() {
        return Err(Error::Compression("A/V frame grew".to_owned()));
    }
    Ok(out)
}

/// `avhuff_encoder::encode_data`.
fn encode_data(source: &[u8]) -> Result<Vec<u8>> {
    let metasize = usize::from(source[4]);
    let channels = usize::from(source[5]);
    let samples = be16(source, 6) as usize;
    let width = be16(source, 8) as usize;
    let height = be16(source, 10) as usize;
    let mut dest = vec![0u8; 10 + 2 * channels];
    dest[..8].copy_from_slice(&source[4..12]);
    let mut source = &source[12..];

    dest.extend_from_slice(&source[..metasize]);
    source = &source[metasize..];

    if channels > 0 {
        // every channel a FLAC stream, the tree size 0xffff saying so
        dest[8] = 0xff;
        dest[9] = 0xff;
        for chnum in 0..channels {
            let samples_bytes = &source[chnum * samples * 2..][..samples * 2];
            let encoded = if samples == 0 {
                Vec::new()
            } else {
                flac_encode_with(samples_bytes, 1, FLAC_SAMPLE_RATE, samples as u32, true)?
            };
            // chdman's buffer holds the raw samples' size; past it, it
            // writes a corrupt frame where this refuses
            if encoded.len() > samples * 2 {
                return Err(Error::Compression(
                    "FLAC failed to compress the audio".to_owned(),
                ));
            }
            dest[10 + 2 * chnum..][..2].copy_from_slice(&(encoded.len() as u16).to_be_bytes());
            dest.extend_from_slice(&encoded);
        }
        source = &source[channels * samples * 2..];
    }

    if width > 0 && height > 0 {
        dest.extend_from_slice(&encode_video(source, width, height)?);
    }
    Ok(dest)
}

/// `encode_video_lossless`: an 0x80 marker byte, the three byte-aligned
/// trees, then the components interleaved as the pixels hold them.
fn encode_video(source: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    let mut out = vec![0u8; width * height * 2];
    let length = {
        let mut bitbuf = BitstreamOut::new(&mut out);
        bitbuf.write(0x80, 8);
        let mut y = DeltaRleEncoder::new(source, width, 2, height);
        let mut cb = DeltaRleEncoder::new(&source[1..], width / 2, 4, height);
        let mut cr = DeltaRleEncoder::new(&source[3..], width / 2, 4, height);
        for context in [&y, &cb, &cr] {
            context.encoder.export_tree_rle(&mut bitbuf)?;
            bitbuf.flush();
        }
        for _ in 0..height {
            y.rlecount = 0;
            cb.rlecount = 0;
            cr.rlecount = 0;
            for _ in 0..width / 2 {
                y.encode_one(&mut bitbuf);
                cb.encode_one(&mut bitbuf);
                y.encode_one(&mut bitbuf);
                cr.encode_one(&mut bitbuf);
            }
        }
        let length = bitbuf.flush();
        // chdman stores the truncated stream here, a corrupt frame
        if bitbuf.overflow() {
            return Err(Error::Compression("video failed to compress".to_owned()));
        }
        length
    };
    out.truncate(length);
    Ok(out)
}

/// Decompresses a frame to its raw form, padding the rest of `dest` with
/// zeros: `chd_avhuff_decompressor::decompress`.
pub(crate) fn decompress(source: &[u8], dest: &mut [u8]) -> Result<()> {
    let invalid = || Error::Corrupt("A/V frame is corrupt".to_owned());
    if source.len() < 8 {
        return Err(invalid());
    }
    let metasize = usize::from(source[0]);
    let channels = usize::from(source[1]);
    let samples = be16(source, 2) as usize;
    let width = be16(source, 4) as usize;
    let height = be16(source, 6) as usize;
    if source.len() < 10 + 2 * channels {
        return Err(invalid());
    }
    let treesize = be16(source, 8) as usize;
    let sizes: Vec<usize> = (0..channels)
        .map(|chnum| be16(source, 10 + 2 * chnum) as usize)
        .collect();
    let total = 10
        + 2 * channels
        + if treesize != 0xffff { treesize } else { 0 }
        + sizes.iter().sum::<usize>();
    if total >= source.len() {
        return Err(invalid());
    }
    let size = 12 + metasize + 2 * channels * samples + 2 * width * height;
    if dest.len() < size {
        return Err(invalid());
    }

    dest[..4].copy_from_slice(b"chav");
    dest[4..12].copy_from_slice(&source[..8]);
    let mut srcoffs = 10 + 2 * channels;
    dest[12..12 + metasize].copy_from_slice(&source[srcoffs..srcoffs + metasize]);
    srcoffs += metasize;

    let audio = 12 + metasize;
    if channels > 0 {
        decode_audio(
            &source[srcoffs..],
            &mut dest[audio..audio + 2 * channels * samples],
            samples,
            treesize,
            &sizes,
        )?;
        if treesize != 0xffff {
            srcoffs += treesize;
        }
        srcoffs += sizes.iter().sum::<usize>();
    }

    let video = audio + 2 * channels * samples;
    if width > 0 && height > 0 {
        decode_video(&source[srcoffs..], &mut dest[video..size], width, height)?;
    }
    dest[size..].fill(0);
    Ok(())
}

/// `decode_audio`, to big-endian samples: FLAC streams, Huffman-coded
/// deltas, or raw deltas, by the tree size.
fn decode_audio(
    source: &[u8],
    dest: &mut [u8],
    samples: usize,
    treesize: usize,
    sizes: &[usize],
) -> Result<()> {
    let invalid = || Error::Corrupt("A/V audio is corrupt".to_owned());
    let mut source = source;
    if treesize == 0xffff {
        for (chnum, &size) in sizes.iter().enumerate() {
            let out = &mut dest[chnum * samples * 2..][..samples * 2];
            if samples > 0 {
                flac_decode_with(
                    source.get(..size).ok_or_else(invalid)?,
                    out,
                    1,
                    FLAC_SAMPLE_RATE,
                    samples as u32,
                    true,
                )?;
            }
            source = &source[size..];
        }
        return Ok(());
    }

    let mut hi = HuffmanDecoder::new(256, MAX_BITS);
    let mut lo = HuffmanDecoder::new(256, MAX_BITS);
    if treesize != 0 {
        let mut bitbuf = BitstreamIn::new(source.get(..treesize).ok_or_else(invalid)?);
        hi.import_tree_rle(&mut bitbuf).map_err(|_| invalid())?;
        bitbuf.flush();
        lo.import_tree_rle(&mut bitbuf).map_err(|_| invalid())?;
        if bitbuf.flush() != treesize {
            return Err(invalid());
        }
        source = &source[treesize..];
    }
    for (chnum, &size) in sizes.iter().enumerate() {
        let out = &mut dest[chnum * samples * 2..][..samples * 2];
        let stream = source.get(..size).ok_or_else(invalid)?;
        let mut prevsample = 0i16;
        if treesize == 0 {
            for (sample, raw) in out.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                let delta = i16::from_be_bytes([stream[sample * 2], stream[sample * 2 + 1]]);
                prevsample = prevsample.wrapping_add(delta);
                *raw = prevsample.to_be_bytes();
            }
        } else {
            let mut bitbuf = BitstreamIn::new(stream);
            for raw in out.as_chunks_mut::<2>().0 {
                let delta = ((hi.decode_one(&mut bitbuf) << 8) | lo.decode_one(&mut bitbuf)) as u16;
                prevsample = prevsample.wrapping_add(delta as i16);
                *raw = prevsample.to_be_bytes();
            }
            if bitbuf.overflow() {
                return Err(invalid());
            }
        }
        source = &source[size..];
    }
    Ok(())
}

/// `decode_video_lossless`: the stream must end exactly where the frame
/// does.
fn decode_video(source: &[u8], dest: &mut [u8], width: usize, height: usize) -> Result<()> {
    let invalid = || Error::Corrupt("A/V video is corrupt".to_owned());
    if source.first().is_none_or(|byte| byte & 0x80 == 0) {
        return Err(invalid());
    }
    let mut bitbuf = BitstreamIn::new(source);
    bitbuf.read(8);
    let mut y = DeltaRleDecoder::new();
    let mut cb = DeltaRleDecoder::new();
    let mut cr = DeltaRleDecoder::new();
    for context in [&mut y, &mut cb, &mut cr] {
        context
            .decoder
            .import_tree_rle(&mut bitbuf)
            .map_err(|_| invalid())?;
        bitbuf.flush();
    }
    for row in dest.chunks_exact_mut(width * 2).take(height) {
        for pixel in row.as_chunks_mut::<4>().0 {
            pixel[0] = y.decode_one(&mut bitbuf);
            pixel[1] = cb.decode_one(&mut bitbuf);
            pixel[2] = y.decode_one(&mut bitbuf);
            pixel[3] = cr.decode_one(&mut bitbuf);
        }
        y.rlecount = 0;
        cb.rlecount = 0;
        cr.rlecount = 0;
    }
    if bitbuf.overflow() || bitbuf.flush() != source.len() {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A raw frame: a smooth gradient with flat runs, and a sine of audio.
    fn frame(width: usize, height: usize, channels: usize, samples: usize) -> Vec<u8> {
        let mut raw = b"chav".to_vec();
        raw.push(0);
        raw.push(channels as u8);
        raw.extend_from_slice(&(samples as u16).to_be_bytes());
        raw.extend_from_slice(&(width as u16).to_be_bytes());
        raw.extend_from_slice(&(height as u16).to_be_bytes());
        for chnum in 0..channels {
            for sample in 0..samples {
                let value = ((sample as f64 / 20.0 + chnum as f64).sin() * 8000.0) as i16;
                raw.extend_from_slice(&value.to_be_bytes());
            }
        }
        for y in 0..height {
            for x in 0..width {
                let luma = if x > width / 2 { 16 } else { (x + y) as u8 };
                raw.push(luma);
                raw.push(128u8.wrapping_add((y / 4) as u8));
            }
        }
        raw
    }

    #[test]
    fn frames_round_trip() {
        let raw = frame(64, 20, 2, 800);
        let mut hunk = raw.clone();
        hunk.resize(raw.len() + 16, 0);
        let packed = compress(&hunk).unwrap();
        assert!(packed.len() < raw.len());
        let mut out = vec![0xaa; hunk.len()];
        decompress(&packed, &mut out).unwrap();
        assert_eq!(out, hunk);
    }

    #[test]
    fn run_lengths_follow_the_codes() {
        assert_eq!(rlecount_to_code(7), 0);
        assert_eq!(rlecount_to_code(8), 0x100);
        assert_eq!(rlecount_to_code(15), 0x107);
        assert_eq!(code_to_rlecount(rlecount_to_code(100_000)), 2048);
        assert_eq!(code_to_rlecount(rlecount_to_code(40)), 32);
    }
}

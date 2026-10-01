//! CHD chunk compression and decompression, ported from MAME's
//! `src/lib/util/chdcodec.cpp`.
//!
//! Each CHD header names up to four codec slots by a big-endian fourcc tag;
//! chunk map entries reference a slot by index. The slots are validated when
//! a file is opened — an unknown tag is rejected there — but instantiated
//! lazily, so metadata stays readable for codecs whose backend this crate
//! does not implement yet.

use std::io::{BufReader, Read};

use crate::bitstream::{BitstreamIn, BitstreamOut};
use crate::ecc::{ecc_clear, ecc_generate, ecc_verify};
use crate::flac::{flac_decode, flac_encode};
use crate::huffman::{HuffmanDecoder, HuffmanEncoder};
use crate::{Error, Result};

pub(crate) const CODEC_NONE: u32 = 0;
pub(crate) const CODEC_ZLIB: u32 = u32::from_be_bytes(*b"zlib");
pub(crate) const CODEC_ZSTD: u32 = u32::from_be_bytes(*b"zstd");
pub(crate) const CODEC_LZMA: u32 = u32::from_be_bytes(*b"lzma");
pub(crate) const CODEC_HUFF: u32 = u32::from_be_bytes(*b"huff");
pub(crate) const CODEC_FLAC: u32 = u32::from_be_bytes(*b"flac");
pub(crate) const CODEC_CD_ZLIB: u32 = u32::from_be_bytes(*b"cdzl");
pub(crate) const CODEC_CD_ZSTD: u32 = u32::from_be_bytes(*b"cdzs");
pub(crate) const CODEC_CD_LZMA: u32 = u32::from_be_bytes(*b"cdlz");
pub(crate) const CODEC_CD_FLAC: u32 = u32::from_be_bytes(*b"cdfl");
pub(crate) const CODEC_AV_HUFF: u32 = u32::from_be_bytes(*b"avhu");

/// A CD frame: 2352 bytes of sector data plus 96 bytes of subcode.
pub(crate) const FRAME_SIZE: usize = 2448;
/// Sector data bytes in a CD frame.
pub(crate) const MAX_SECTOR_DATA: usize = 2352;
/// Subcode bytes in a CD frame.
pub(crate) const MAX_SUBCODE_DATA: usize = 96;
/// The sync header a raw CD sector starts with.
pub(crate) const SYNC_HEADER: [u8; 12] = [
    0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00,
];

/// Whether a file naming this codec can be opened at all.
pub(crate) fn is_known(tag: u32) -> bool {
    matches!(
        tag,
        CODEC_ZLIB
            | CODEC_ZSTD
            | CODEC_LZMA
            | CODEC_HUFF
            | CODEC_FLAC
            | CODEC_CD_ZLIB
            | CODEC_CD_ZSTD
            | CODEC_CD_LZMA
            | CODEC_CD_FLAC
            | CODEC_AV_HUFF
    )
}

/// The friendly name of a codec, from MAME's codec list.
pub(crate) fn name(tag: u32) -> Option<&'static str> {
    Some(match tag {
        CODEC_NONE => return None,
        CODEC_ZLIB => "Deflate",
        CODEC_ZSTD => "Zstandard",
        CODEC_LZMA => "LZMA",
        CODEC_HUFF => "Huffman",
        CODEC_FLAC => "FLAC",
        CODEC_CD_ZLIB => "CD Deflate",
        CODEC_CD_ZSTD => "CD Zstandard",
        CODEC_CD_LZMA => "CD LZMA",
        CODEC_CD_FLAC => "CD FLAC",
        CODEC_AV_HUFF => "A/V Huffman",
        _ => "unknown",
    })
}

/// The four ASCII characters of a codec tag.
pub(crate) fn fourcc(tag: u32) -> String {
    let bytes = tag.to_be_bytes();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The codec list of a header as a display string, like chdman's
/// `compression_string`.
pub(crate) fn compression_string(compression: &[u32; 4]) -> String {
    if compression[0] == CODEC_NONE {
        return "none".to_string();
    }
    let mut parts = Vec::new();
    for &tag in compression {
        if tag == CODEC_NONE {
            break;
        }
        let mut part = fourcc(tag);
        if let Some(name) = name(tag) {
            part.push_str(" (");
            part.push_str(name);
            part.push(')');
        }
        parts.push(part);
    }
    parts.join(", ")
}

/// A decoder for one codec slot of an open CHD.
pub(crate) enum Decompressor {
    /// `CHD_CODEC_NONE`: stored verbatim.
    None,
    /// `CHD_CODEC_ZLIB`: raw deflate.
    Zlib,
    /// `CHD_CODEC_ZSTD`: a Zstandard frame.
    Zstd,
    /// `CHD_CODEC_LZMA`: a raw LZMA1 stream.
    Lzma { dict_size: u32 },
    /// `CHD_CODEC_HUFF`: a Huffman tree followed by symbols.
    Huff,
    /// `CHD_CODEC_FLAC`: a headerless FLAC stream of 44.1 kHz stereo
    /// samples, prefixed by an endian flag byte.
    Flac,
    /// `CHD_CODEC_CD_FLAC`: FLAC for the sector data plane and raw deflate
    /// for the subcode plane, with no sync or ECC bits.
    CdFlac,
    /// The `cd??` codecs: two independently compressed planes, sector data
    /// and subcode, plus one bit per frame recording whether its sync
    /// header and ECC were stored or can be regenerated.
    Cd {
        base: Box<Decompressor>,
        sub: Box<Decompressor>,
    },
}

impl Decompressor {
    /// Instantiates the decoder for a codec tag, like
    /// `chd_codec_list::new_decompressor`. Codecs this crate does not decode
    /// yet report [`Error::Unsupported`].
    pub fn create(tag: u32, hunkbytes: u32) -> Result<Self> {
        Ok(match tag {
            CODEC_NONE => Self::None,
            CODEC_ZLIB => Self::Zlib,
            CODEC_ZSTD => Self::Zstd,
            CODEC_LZMA => Self::Lzma {
                dict_size: lzma_dict_size(hunkbytes),
            },
            CODEC_HUFF => Self::Huff,
            CODEC_FLAC => Self::Flac,
            CODEC_CD_ZLIB => Self::Cd {
                base: Box::new(Self::Zlib),
                sub: Box::new(Self::Zlib),
            },
            CODEC_CD_ZSTD => Self::Cd {
                base: Box::new(Self::Zstd),
                sub: Box::new(Self::Zstd),
            },
            CODEC_CD_LZMA => Self::Cd {
                base: Box::new(Self::Lzma {
                    dict_size: lzma_dict_size(hunkbytes),
                }),
                sub: Box::new(Self::Zlib),
            },
            CODEC_CD_FLAC => Self::CdFlac,
            CODEC_AV_HUFF => Self::unsupported(tag)?,
            _ => {
                return Err(Error::Unsupported(format!(
                    "unknown compression `{}`",
                    fourcc(tag)
                )));
            }
        })
    }

    fn unsupported(tag: u32) -> Result<Self> {
        Err(Error::Unsupported(format!(
            "{} compression is not supported",
            name(tag).unwrap_or("unknown")
        )))
    }

    /// Decompresses `source` into `dest`.
    pub fn decompress(&mut self, source: &[u8], dest: &mut [u8]) -> Result<()> {
        match self {
            Self::None => {
                if source.len() < dest.len() {
                    return Err(Error::Corrupt("stored chunk is truncated".to_string()));
                }
                dest.copy_from_slice(&source[..dest.len()]);
                Ok(())
            }
            Self::Zlib => {
                let mut reader = BufReader::new(source);
                flate2::bufread::DeflateDecoder::new(&mut reader)
                    .read_exact(dest)
                    .map_err(|_| Error::Corrupt("zlib chunk is corrupt".to_string()))
            }
            Self::Zstd => {
                let mut reader = BufReader::new(source);
                zstd::stream::Decoder::new(&mut reader)
                    .map_err(|_| Error::Corrupt("zstd chunk is corrupt".to_string()))?
                    .read_exact(dest)
                    .map_err(|_| Error::Corrupt("zstd chunk is corrupt".to_string()))
            }
            Self::Lzma { dict_size } => Self::decompress_lzma(source, dest, *dict_size),
            Self::Huff => Self::decompress_huff(source, dest),
            Self::Flac => Self::decompress_flac(source, dest),
            Self::CdFlac => Self::decompress_cd_flac(source, dest),
            Self::Cd { base, sub } => Self::decompress_cd(source, dest, base, sub),
        }
    }

    pub(crate) fn decompress_lzma(source: &[u8], dest: &mut [u8], dict_size: u32) -> Result<()> {
        // CHD files store a raw LZMA1 stream with no properties byte: MAME
        // reconstructs the encoder properties from the chunk size, fixing
        // lc/lp/pb at 3/0/2 and clamping the dictionary.
        let properties = lzma_rs::decompress::raw::LzmaProperties {
            lc: 3,
            lp: 0,
            pb: 2,
        };
        let params = lzma_rs::decompress::raw::LzmaParams::new(
            properties,
            dict_size,
            Some(dest.len() as u64),
        );
        let mut decoder = lzma_rs::decompress::raw::LzmaDecoder::new(params, None)
            .map_err(|_| Error::Corrupt("lzma chunk is corrupt".to_string()))?;
        let mut reader = BufReader::new(source);
        let mut out: Vec<u8> = Vec::with_capacity(dest.len());
        decoder
            .decompress(&mut reader, &mut out)
            .map_err(|_| Error::Corrupt("lzma chunk is corrupt".to_string()))?;
        if out.len() < dest.len() {
            return Err(Error::Corrupt("lzma chunk is truncated".to_string()));
        }
        dest.copy_from_slice(&out[..dest.len()]);
        Ok(())
    }

    fn decompress_huff(source: &[u8], dest: &mut [u8]) -> Result<()> {
        // Each chunk carries its own Huffman tree.
        let mut bitbuf = BitstreamIn::new(source);
        let mut huff = HuffmanDecoder::new(256, 16);
        huff.import_tree_huffman(&mut bitbuf)?;
        for byte in dest.iter_mut() {
            *byte = huff.decode_one(&mut bitbuf) as u8;
        }
        if bitbuf.overflow() {
            return Err(Error::Corrupt("huffman chunk is truncated".to_string()));
        }
        Ok(())
    }

    fn decompress_cd(
        source: &[u8],
        dest: &mut [u8],
        base: &mut Decompressor,
        sub: &mut Decompressor,
    ) -> Result<()> {
        let frames = dest.len() / FRAME_SIZE;
        if frames == 0 || frames * FRAME_SIZE != dest.len() {
            return Err(Error::Corrupt(
                "CD chunk size is not a whole number of frames".to_string(),
            ));
        }
        // Header: one bit per frame for the ECC flag, then the length of the
        // base plane.
        let sync_bytes = frames.div_ceil(8);
        let length_bytes = if dest.len() < 65536 { 2 } else { 3 };
        let header_bytes = sync_bytes + length_bytes;
        if source.len() < header_bytes {
            return Err(Error::Corrupt("CD chunk header is truncated".to_string()));
        }
        let base_len = if length_bytes > 2 {
            u32::from_be_bytes([
                0,
                source[sync_bytes],
                source[sync_bytes + 1],
                source[sync_bytes + 2],
            ])
        } else {
            u32::from(u16::from_be_bytes([
                source[sync_bytes],
                source[sync_bytes + 1],
            ]))
        };
        let base_end = header_bytes + base_len as usize;
        if source.len() < base_end {
            return Err(Error::Corrupt("CD chunk is truncated".to_string()));
        }
        // Decompress the two planes back to back in one buffer.
        let sector_bytes = frames * MAX_SECTOR_DATA;
        let subcode_bytes = frames * MAX_SUBCODE_DATA;
        let mut buffer = vec![0u8; sector_bytes + subcode_bytes];
        base.decompress(&source[header_bytes..base_end], &mut buffer[..sector_bytes])?;
        sub.decompress(&source[base_end..], &mut buffer[sector_bytes..])?;
        // Interleave, regenerating sync and ECC where flagged.
        for framenum in 0..frames {
            let frame = &mut dest[framenum * FRAME_SIZE..][..FRAME_SIZE];
            frame[..MAX_SECTOR_DATA]
                .copy_from_slice(&buffer[framenum * MAX_SECTOR_DATA..][..MAX_SECTOR_DATA]);
            frame[MAX_SECTOR_DATA..].copy_from_slice(
                &buffer[sector_bytes + framenum * MAX_SUBCODE_DATA..][..MAX_SUBCODE_DATA],
            );
            if source[framenum / 8] & (1 << (framenum % 8)) != 0 {
                frame[..SYNC_HEADER.len()].copy_from_slice(&SYNC_HEADER);
                ecc_generate(&mut frame[..MAX_SECTOR_DATA]);
            }
        }
        Ok(())
    }

    fn decompress_flac(source: &[u8], dest: &mut [u8]) -> Result<()> {
        // The plain FLAC compressor encodes the hunk twice, once reading
        // its samples as big-endian and once as little-endian, and keeps
        // the smaller output behind a flag byte naming which it kept.
        let big_endian = match source.first() {
            Some(b'B') => true,
            Some(b'L') => false,
            _ => {
                return Err(Error::Corrupt(
                    "FLAC chunk is missing its endian flag".to_string(),
                ));
            }
        };
        flac_decode(&source[1..], dest, flac_block_size(dest.len()), big_endian)?;
        Ok(())
    }

    fn decompress_cd_flac(source: &[u8], dest: &mut [u8]) -> Result<()> {
        let frames = dest.len() / FRAME_SIZE;
        if frames == 0 || frames * FRAME_SIZE != dest.len() {
            return Err(Error::Corrupt(
                "CD chunk size is not a whole number of frames".to_string(),
            ));
        }
        // The FLAC stream holds the sector data plane, read as big-endian
        // samples; the subcode plane follows it, raw deflated.
        let sector_bytes = frames * MAX_SECTOR_DATA;
        let subcode_bytes = frames * MAX_SUBCODE_DATA;
        let mut buffer = vec![0u8; sector_bytes + subcode_bytes];
        let offset = flac_decode(
            source,
            &mut buffer[..sector_bytes],
            cd_flac_block_size(sector_bytes),
            true,
        )?;
        let mut reader = BufReader::new(&source[offset.min(source.len())..]);
        flate2::bufread::DeflateDecoder::new(&mut reader)
            .read_exact(&mut buffer[sector_bytes..])
            .map_err(|_| Error::Corrupt("CD FLAC subcode chunk is corrupt".to_string()))?;
        for framenum in 0..frames {
            let frame = &mut dest[framenum * FRAME_SIZE..][..FRAME_SIZE];
            frame[..MAX_SECTOR_DATA]
                .copy_from_slice(&buffer[framenum * MAX_SECTOR_DATA..][..MAX_SECTOR_DATA]);
            frame[MAX_SECTOR_DATA..].copy_from_slice(
                &buffer[sector_bytes + framenum * MAX_SUBCODE_DATA..][..MAX_SUBCODE_DATA],
            );
        }
        Ok(())
    }
}

/// The FLAC block size MAME derives for a hunk of `bytes`: the number of
/// stereo samples it holds, halved until it fits the encoder's maximum.
fn flac_block_size(bytes: usize) -> u32 {
    let mut block_size = (bytes / 4) as u32;
    while block_size > 2048 {
        block_size /= 2;
    }
    block_size
}

/// The block size the CD FLAC compressor derives, the same rule with the
/// maximum raised to the sector data bytes of a chunk.
fn cd_flac_block_size(bytes: usize) -> u32 {
    let mut block_size = (bytes / 4) as u32;
    while block_size > MAX_SECTOR_DATA as u32 {
        block_size /= 2;
    }
    block_size
}

/// The LZMA dictionary size MAME derives for a chunk of `hunkbytes`: the
/// level 6 default clamped to the uncompressed size, with a 4096 floor.
pub(crate) fn lzma_dict_size(hunkbytes: u32) -> u32 {
    (1u32 << 26).min(hunkbytes.max(4096))
}

/// Compresses one chunk with a single codec, mirroring
/// `chd_compressor::compress`. An error means the codec declines the
/// chunk, which only keeps its slot from winning; known codecs this crate
/// cannot write yet always decline.
fn compress_chunk(tag: u32, source: &[u8]) -> Result<Vec<u8>> {
    match tag {
        CODEC_ZLIB => compress_zlib(source),
        CODEC_ZSTD => compress_zstd(source),
        CODEC_LZMA => crate::lzma::compress(source, source.len()),
        CODEC_HUFF => compress_huff(source),
        CODEC_FLAC => compress_flac(source),
        CODEC_CD_ZLIB => compress_cd(source, CODEC_ZLIB, CODEC_ZLIB),
        CODEC_CD_ZSTD => compress_cd(source, CODEC_ZSTD, CODEC_ZSTD),
        CODEC_CD_LZMA => compress_cd(source, CODEC_LZMA, CODEC_ZLIB),
        CODEC_CD_FLAC => compress_cd_flac(source),
        _ => Err(Error::Unsupported(format!(
            "compression `{}` cannot be written yet",
            fourcc(tag)
        ))),
    }
}

/// Raw deflate at level 9, as the `zlib_compressor` and the cdfl subcode
/// encoder both run it, without the plain codec's size check.
fn deflate_raw(source: &[u8]) -> Result<Vec<u8>> {
    let mut compressor = flate2::Compress::new(flate2::Compression::new(9), false);
    // A buffer the size of the input, as in chdman: a chunk whose deflate
    // does not fit in it would be declined anyway.
    let mut out = vec![0; source.len().max(1)];
    let mut read = 0;
    let mut written = 0;
    loop {
        let in_before = compressor.total_in();
        let out_before = compressor.total_out();
        let status = compressor
            .compress(
                &source[read..],
                &mut out[written..],
                flate2::FlushCompress::Finish,
            )
            .map_err(|error| {
                Error::Compression(format!("deflate failed to compress the chunk: {error}"))
            })?;
        read += usize::try_from(compressor.total_in() - in_before).unwrap_or(usize::MAX);
        written += usize::try_from(compressor.total_out() - out_before).unwrap_or(usize::MAX);
        match status {
            flate2::Status::StreamEnd => {
                out.truncate(written);
                return Ok(out);
            }
            flate2::Status::Ok if written < out.len() => {}
            _ => {
                return Err(Error::Compression(
                    "deflate failed to compress the chunk".to_owned(),
                ));
            }
        }
    }
}

/// Raw deflate at level 9, the `zlib_compressor`: an output as big as the
/// input is a failure.
fn compress_zlib(source: &[u8]) -> Result<Vec<u8>> {
    let out = deflate_raw(source)?;
    if out.len() >= source.len() {
        return Err(Error::Compression(
            "deflate failed to compress the chunk".to_owned(),
        ));
    }
    Ok(out)
}

/// A zstd frame at the maximum compression level, the `zstd_compressor`:
/// an output as big as the input is a failure.
fn compress_zstd(source: &[u8]) -> Result<Vec<u8>> {
    let level = zstd::zstd_safe::max_c_level();
    let out = zstd::stream::encode_all(source, level).map_err(|error| {
        Error::Compression(format!("zstd failed to compress the chunk: {error}"))
    })?;
    if out.len() >= source.len() {
        return Err(Error::Compression(
            "zstd failed to compress the chunk".to_owned(),
        ));
    }
    Ok(out)
}

/// Huffman-codes one chunk with an 8-bit alphabet, the
/// `huffman_8bit_encoder`: histogram, canonical tree, tree export and the
/// bit-packed symbols, in a buffer the size of the input.
fn compress_huff(source: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = HuffmanEncoder::new(256, 16);
    for &byte in source {
        encoder.histo_one(usize::from(byte));
    }
    encoder.compute_tree_from_histo()?;
    let mut out = vec![0; source.len()];
    let complen = {
        let mut bitbuf = BitstreamOut::new(&mut out);
        encoder.export_tree_huffman(&mut bitbuf)?;
        for &byte in source {
            encoder.encode_one(&mut bitbuf, usize::from(byte));
        }
        let complen = bitbuf.flush();
        if bitbuf.overflow() {
            return Err(Error::Compression(
                "huffman coding failed to compress the chunk".to_owned(),
            ));
        }
        complen
    };
    out.truncate(complen);
    Ok(out)
}

/// FLAC-codes one hunk of plain data, the `chd_flac_compressor`: the hunk
/// is encoded twice, once reading its bytes as big-endian and once as
/// little-endian 16-bit stereo samples, and the smaller stream wins,
/// flagged by its first byte so the decompressor reads the samples back
/// the same way.
fn compress_flac(source: &[u8]) -> Result<Vec<u8>> {
    let block_size = flac_block_size(source.len());
    let big = flac_encode(source, block_size, true)?;
    let little = flac_encode(source, block_size, false)?;
    let (flag, payload) = if little.len() <= big.len() {
        (b'L', little)
    } else {
        (b'B', big)
    };
    if payload.len() + 1 >= source.len() {
        return Err(Error::Compression(
            "FLAC failed to compress the chunk".to_owned(),
        ));
    }
    let mut out = Vec::with_capacity(payload.len() + 1);
    out.push(flag);
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Compresses one hunk of CD frames, the `chd_cd_compressor` template
/// instantiated by the codec tag: the frames split into their sector data
/// and subcode planes, frames that are a valid sync-bearing data sector
/// have that part of them dropped in favour of a bit in a leading ECC
/// bitmap, the sector data plane goes to the base codec and the subcode
/// plane to the sub codec, and the two are concatenated behind a header of
/// the bitmap plus the big-endian byte count of the base encoding.
fn compress_cd(source: &[u8], base_tag: u32, sub_tag: u32) -> Result<Vec<u8>> {
    if source.is_empty() || !source.len().is_multiple_of(FRAME_SIZE) {
        return Err(Error::Compression(
            "CD chunk input is not a whole number of frames".to_owned(),
        ));
    }
    let frames = source.len() / FRAME_SIZE;
    let ecc_bytes = frames.div_ceil(8);
    let complen_bytes = if source.len() < 65_536 { 2 } else { 3 };
    let mut dest = vec![0; ecc_bytes];
    let mut plane = vec![0; frames * MAX_SECTOR_DATA];
    let mut subcode = vec![0; frames * MAX_SUBCODE_DATA];
    for (framenum, frame) in source.as_chunks::<FRAME_SIZE>().0.iter().enumerate() {
        let sector = &mut plane[framenum * MAX_SECTOR_DATA..][..MAX_SECTOR_DATA];
        sector.copy_from_slice(&frame[..MAX_SECTOR_DATA]);
        subcode[framenum * MAX_SUBCODE_DATA..][..MAX_SUBCODE_DATA]
            .copy_from_slice(&frame[MAX_SECTOR_DATA..]);
        // A recoverable data sector: the sync header and its ECC bytes are
        // implied by the flag bit and regenerated when the chunk is read.
        if sector[..SYNC_HEADER.len()] == SYNC_HEADER && ecc_verify(sector) {
            dest[framenum / 8] |= 1 << (framenum % 8);
            sector[..SYNC_HEADER.len()].fill(0);
            ecc_clear(sector);
        }
    }
    let base = compress_chunk(base_tag, &plane)?;
    if base.len() >= source.len() {
        return Err(Error::Compression(format!(
            "{} failed to compress the CD chunk",
            fourcc(base_tag)
        )));
    }
    let Ok(complen) = u32::try_from(base.len()) else {
        return Err(Error::Compression(format!(
            "{} produced an oversized CD chunk",
            fourcc(base_tag)
        )));
    };
    dest.extend_from_slice(&complen.to_be_bytes()[4 - complen_bytes..]);
    dest.extend_from_slice(&base);
    dest.extend_from_slice(&compress_chunk(sub_tag, &subcode)?);
    Ok(dest)
}

/// FLAC-codes one hunk of CD frames, the `chd_cd_flac_compressor`: the
/// frames split into their planes like the other CD codecs but with no
/// ECC bitmap, the sector data plane FLAC-encoded with the samples read
/// big-endian as the encoder always does here, and the subcode plane raw
/// deflated after it. The FLAC stream self-terminates; there is no header.
fn compress_cd_flac(source: &[u8]) -> Result<Vec<u8>> {
    if source.is_empty() || !source.len().is_multiple_of(FRAME_SIZE) {
        return Err(Error::Compression(
            "CD chunk input is not a whole number of frames".to_owned(),
        ));
    }
    let frames = source.len() / FRAME_SIZE;
    let mut plane = vec![0; frames * MAX_SECTOR_DATA];
    let mut subcode = vec![0; frames * MAX_SUBCODE_DATA];
    for (framenum, frame) in source.as_chunks::<FRAME_SIZE>().0.iter().enumerate() {
        plane[framenum * MAX_SECTOR_DATA..][..MAX_SECTOR_DATA]
            .copy_from_slice(&frame[..MAX_SECTOR_DATA]);
        subcode[framenum * MAX_SUBCODE_DATA..][..MAX_SUBCODE_DATA]
            .copy_from_slice(&frame[MAX_SECTOR_DATA..]);
    }
    // chdman sizes the encoder from the plane bytes, frames × sector data.
    let mut out = flac_encode(&plane, cd_flac_block_size(frames * MAX_SECTOR_DATA), true)?;
    out.extend_from_slice(&deflate_raw(&subcode)?);
    if out.len() >= source.len() {
        return Err(Error::Compression(
            "FLAC failed to compress the CD chunk".to_owned(),
        ));
    }
    Ok(out)
}

/// Compresses one chunk with each codec slot in turn and keeps the
/// smallest encoding, porting `chd_compressor_group::find_best_compressor`
/// — errors are swallowed and ties go to the earliest slot. Returns the
/// winning slot index and its bytes, or `-1` and the raw chunk when no
/// codec fits.
pub(crate) fn find_best_compressor(compression: &[u32; 4], source: &[u8]) -> (i8, Vec<u8>) {
    let mut best: i8 = -1;
    let mut complen = source.len();
    let mut compressed = Vec::new();
    for (index, &tag) in compression.iter().enumerate() {
        if tag == CODEC_NONE {
            continue;
        }
        // MAME catches every compression error here and moves on.
        if let Ok(out) = compress_chunk(tag, source)
            && out.len() < complen
        {
            best = index as i8;
            complen = out.len();
            compressed = out;
        }
    }
    if best == -1 {
        compressed = source.to_vec();
    }
    (best, compressed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecc::{ecc_generate, ecc_verify};

    /// A synthetic CD hunk: `frames` audio-like frames of smooth samples
    /// with silent subcode, and frame 3 replaced by a mode-1 data sector
    /// carrying valid ECC.
    fn cd_frames(frames: usize) -> Vec<u8> {
        // A triangle wave repeating over one whole frame of 612 samples:
        // every frame's plane and subcode bytes are then identical, and
        // the smooth ramps keep FLAC's linear predictor happy.
        fn triangle(i: usize) -> i16 {
            let ramp = (i % 612) as i16;
            if ramp < 306 {
                ramp * 100
            } else {
                (611 - ramp) * 100
            }
        }
        let mut hunk = vec![0u8; frames * FRAME_SIZE];
        for (i, sample) in hunk.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let left = triangle(i);
            let right = triangle(i + 306);
            sample[..2].copy_from_slice(&left.to_be_bytes());
            sample[2..].copy_from_slice(&right.to_be_bytes());
        }
        let sector = &mut hunk[3 * FRAME_SIZE..3 * FRAME_SIZE + MAX_SECTOR_DATA];
        sector[..SYNC_HEADER.len()].copy_from_slice(&SYNC_HEADER);
        sector[0x0f] = 1;
        for byte in sector.iter_mut().skip(16) {
            *byte = byte.wrapping_mul(31).wrapping_add(7);
        }
        ecc_generate(sector);
        assert!(ecc_verify(sector));
        hunk
    }

    #[test]
    fn cd_codecs_round_trip_frames_and_their_ecc_flags() {
        let hunk = cd_frames(8);
        let hunkbytes = u32::try_from(hunk.len()).unwrap();
        for tag in [CODEC_CD_ZLIB, CODEC_CD_ZSTD, CODEC_CD_LZMA, CODEC_CD_FLAC] {
            let packed = compress_chunk(tag, &hunk).unwrap();
            assert!(packed.len() < hunk.len(), "{tag} expanded the hunk");
            // The frame with sync bytes and valid ECC has its ECC cleared
            // and flagged in the bitmap that opens the zlib and lzma streams.
            if tag != CODEC_CD_FLAC {
                assert_ne!(packed[0] & (1 << 3), 0, "{tag} flagged no ECC");
                assert_eq!(packed[0] & !(1 << 3), 0, "{tag} flagged more frames");
            }
            let mut out = vec![0u8; hunk.len()];
            Decompressor::create(tag, hunkbytes)
                .unwrap()
                .decompress(&packed, &mut out)
                .unwrap();
            assert_eq!(out, hunk, "{tag}");
        }
    }

    #[test]
    fn flac_round_trips_a_cd_hunk() {
        let hunk = cd_frames(8);
        let packed = compress_chunk(CODEC_FLAC, &hunk).unwrap();
        let mut out = vec![0u8; hunk.len()];
        Decompressor::create(CODEC_FLAC, u32::try_from(hunk.len()).unwrap())
            .unwrap()
            .decompress(&packed, &mut out)
            .unwrap();
        assert_eq!(out, hunk);
    }

    /// High-entropy pseudo-random bytes, incompressible for our codecs.
    fn random_bytes() -> Vec<u8> {
        let mut random = Vec::new();
        let mut state = 0x2545_F491_C16B_9EE1u64;
        while random.len() < 4096 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            random.extend_from_slice(&state.to_le_bytes());
        }
        random
    }

    /// Compressible and incompressible chunks, the sizes of a hunk.
    fn samples() -> Vec<Vec<u8>> {
        let mut text = Vec::new();
        while text.len() < 4096 {
            text.extend_from_slice(b"CHD-RS compression round-trip sample. ");
        }
        text.truncate(4096);
        let mut mixed = text.clone();
        mixed[2048..].fill(0);
        vec![vec![0; 4096], text, mixed, random_bytes()]
    }

    #[test]
    fn compressors_roundtrip_through_their_decompressors() {
        for tag in [CODEC_ZLIB, CODEC_ZSTD, CODEC_LZMA, CODEC_HUFF] {
            for source in samples() {
                match compress_chunk(tag, &source) {
                    Ok(packed) => {
                        assert!(packed.len() < source.len(), "{tag} expanded its input");
                        let mut decompressed = vec![0; source.len()];
                        Decompressor::create(tag, source.len() as u32)
                            .unwrap()
                            .decompress(&packed, &mut decompressed)
                            .unwrap();
                        assert_eq!(decompressed, source);
                    }
                    Err(Error::Compression(_)) => (),
                    Err(error) => panic!("unexpected error: {error}"),
                }
            }
        }
    }

    #[test]
    fn find_best_compressor_stores_chunks_no_codec_can_shrink() {
        let source = random_bytes();
        for slot in 0..4 {
            let mut compression = [CODEC_NONE; 4];
            compression[slot] = CODEC_ZSTD;
            let (best, packed) = find_best_compressor(&compression, &source);
            assert_eq!(best, -1);
            assert_eq!(packed, source);
        }
    }

    #[test]
    fn find_best_compressor_picks_a_slot_and_beats_the_raw_size() {
        let compression = [CODEC_ZLIB, CODEC_LZMA, CODEC_HUFF, CODEC_ZSTD];
        for source in samples() {
            let (best, packed) = find_best_compressor(&compression, &source);
            if packed == source {
                assert_eq!(best, -1);
                continue;
            }
            assert_ne!(best, -1);
            assert!(packed.len() < source.len());
            let mut decompressed = vec![0; source.len()];
            Decompressor::create(compression[best as usize], source.len() as u32)
                .unwrap()
                .decompress(&packed, &mut decompressed)
                .unwrap();
            assert_eq!(decompressed, source);
        }
    }
}

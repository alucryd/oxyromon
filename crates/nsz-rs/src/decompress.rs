//! NCZ decompression: solid and block, reconstructing the encrypted NCA and its
//! SHA-256. Mirrors `nsz/Decompressor.py::__decompressNcz` and
//! `nsz/BlockDecompressorReader.py`.

use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};

use sha2::{Digest, Sha256};

use crate::crypto::ctr;
use crate::error::{Error, Result};
use crate::format::nca::is_ctr;
use crate::format::ncz::{self, BlockHeader, Section, INCOMPRESSIBLE_HEADER_SIZE};

const CHUNK: usize = 0x100000;

/// Sequential reader over independently-compressed NCZ blocks. Keeps the
/// current decompressed block cached so small reads don't re-decompress it.
struct BlockReader<R: Read + Seek> {
    reader: R,
    block_size: usize,
    compressed_offsets: Vec<u64>,
    compressed_sizes: Vec<u32>,
    decompressed_size: u64,
    position: u64,
    cached: Option<(usize, Vec<u8>)>,
}

impl<R: Read + Seek> BlockReader<R> {
    fn new(reader: R, bh: &BlockHeader, payload_start: u64) -> Result<BlockReader<R>> {
        if !(14..=32).contains(&bh.block_size_exponent) {
            return Err(Error::Corrupt(
                "NCZBLOCK: block size exponent must be 14..=32".into(),
            ));
        }
        let offsets = bh
            .compressed_block_size_list
            .iter()
            .scan(payload_start, |off, &sz| {
                let start = *off;
                *off += sz as u64;
                Some(start)
            })
            .collect();
        Ok(BlockReader {
            reader,
            block_size: 1usize << bh.block_size_exponent,
            compressed_offsets: offsets,
            compressed_sizes: bh.compressed_block_size_list.clone(),
            decompressed_size: bh.decompressed_size as u64,
            position: 0,
            cached: None,
        })
    }

    fn decompress_block(&mut self, block_id: usize) -> io::Result<Vec<u8>> {
        let mut decompressed_block_size = self.block_size;
        if block_id + 1 == self.compressed_offsets.len() {
            let remainder = (self.decompressed_size % self.block_size as u64) as usize;
            if remainder > 0 {
                decompressed_block_size = remainder;
            }
        }
        self.reader
            .seek(SeekFrom::Start(self.compressed_offsets[block_id]))?;
        let csize = self.compressed_sizes[block_id] as usize;
        if csize < decompressed_block_size {
            let mut comp = vec![0u8; csize];
            self.reader.read_exact(&mut comp)?;
            zstd::bulk::decompress(&comp, decompressed_block_size)
        } else {
            let mut raw = vec![0u8; decompressed_block_size];
            self.reader.read_exact(&mut raw)?;
            Ok(raw)
        }
    }
}

impl<R: Read + Seek> Read for BlockReader<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let block_id = (self.position / self.block_size as u64) as usize;
        if self.position >= self.decompressed_size || block_id >= self.compressed_offsets.len() {
            return Ok(0);
        }
        if self.cached.as_ref().map(|(id, _)| *id) != Some(block_id) {
            self.cached = Some((block_id, self.decompress_block(block_id)?));
        }
        let block = &self.cached.as_ref().unwrap().1;
        let offset = (self.position % self.block_size as u64) as usize;
        let n = out.len().min(block.len().saturating_sub(offset));
        out[..n].copy_from_slice(&block[offset..offset + n]);
        self.position += n as u64;
        Ok(n)
    }
}

/// Read the NCZ header region (section table + optional block header) from a
/// seekable reader positioned at file offset 0. Returns the sections (with the
/// incompressible-gap fake section inserted), the block header if present, and the
/// file offset where the compressed payload begins.
pub fn read_ncz_header<R: Read + Seek>(
    f: &mut R,
) -> Result<(Vec<Section>, Option<BlockHeader>, u64)> {
    f.seek(SeekFrom::Start(INCOMPRESSIBLE_HEADER_SIZE))?;
    let mut head = [0u8; 16];
    f.read_exact(&mut head)?;
    let n = usize::try_from(ncz::le_i64(&head, 8))
        .map_err(|_| Error::Corrupt("negative section count".into()))?;
    let mut buf = head.to_vec();
    buf.resize(16 + n * Section::WIRE_SIZE, 0);
    f.read_exact(&mut buf[16..])?;
    let (mut sections, _) = ncz::parse_header(&buf)?;
    let block_start = INCOMPRESSIBLE_HEADER_SIZE + buf.len() as u64;

    // Peek for NCZBLOCK.
    let mut fixed = [0u8; BlockHeader::FIXED_SIZE];
    let has_block = f.read_exact(&mut fixed).is_ok() && fixed[..8] == *ncz::BLOCK_MAGIC;
    let (block, payload_start) = if has_block {
        let nblocks = usize::try_from(ncz::le_i32(&fixed, 12))
            .map_err(|_| Error::Corrupt("negative block count".into()))?;
        let mut bh = fixed.to_vec();
        bh.resize(BlockHeader::FIXED_SIZE + nblocks * 4, 0);
        f.read_exact(&mut bh[BlockHeader::FIXED_SIZE..])?;
        (Some(BlockHeader::read(&bh)?), block_start + bh.len() as u64)
    } else {
        (None, block_start)
    };

    // Insert the fake section covering the gap between the incompressible header
    // and the first real section (stored uncompressed in the stream).
    if !sections.is_empty() && sections[0].offset > INCOMPRESSIBLE_HEADER_SIZE as i64 {
        let fake = Section {
            offset: INCOMPRESSIBLE_HEADER_SIZE as i64,
            size: sections[0].offset - INCOMPRESSIBLE_HEADER_SIZE as i64,
            crypto_type: 0,
            crypto_key: [0u8; 16],
            crypto_counter: [0u8; 16],
        };
        sections.insert(0, fake);
    }

    Ok((sections, block, payload_start))
}

/// Decompress an NCZ (from a seekable reader) into `out`, returning the number of
/// bytes written and the hex SHA-256 of the reconstructed NCA.
pub fn decompress_ncz<R: Read + Seek, W: Write>(f: &mut R, out: &mut W) -> Result<(u64, String)> {
    // The incompressible 0x4000 header is copied verbatim and hashed.
    f.seek(SeekFrom::Start(0))?;
    let mut header = vec![0u8; INCOMPRESSIBLE_HEADER_SIZE as usize];
    f.read_exact(&mut header)?;
    out.write_all(&header)?;
    let mut hasher = Sha256::new();
    hasher.update(&header);

    let (sections, block, payload_start) = read_ncz_header(f)?;
    f.seek(SeekFrom::Start(payload_start))?;
    let body: Box<dyn Read + '_> = match block.as_ref() {
        Some(bh) => Box::new(BlockReader::new(f.by_ref(), bh, payload_start)?),
        None => Box::new(
            zstd::stream::read::Decoder::new(BufReader::new(f.by_ref()))
                .map_err(|e| Error::Corrupt(format!("zstd init: {e}")))?,
        ),
    };

    let body_written = decompress_body(body, &sections, out, &mut hasher)?;
    Ok((
        INCOMPRESSIBLE_HEADER_SIZE + body_written,
        hex::encode(hasher.finalize()),
    ))
}

/// Core loop shared by solid and block: read the decrypted body sequentially,
/// re-encrypt each CTR section, write, and hash the reconstructed NCA body.
fn decompress_body<S: Read, W: Write>(
    mut body: S,
    sections: &[Section],
    out: &mut W,
    hasher: &mut Sha256,
) -> Result<u64> {
    let mut written: u64 = 0;
    let mut chunk = vec![0u8; CHUNK];
    for (idx, s) in sections.iter().enumerate() {
        let mut i = s.offset;
        let end = s.offset + s.size;
        if idx == 0 {
            // The part of the first section inside the 0x4000 header was stored verbatim.
            i = i.max(INCOMPRESSIBLE_HEADER_SIZE as i64);
        }
        while i < end {
            let want = std::cmp::min(CHUNK as i64, end - i) as usize;
            let buf = &mut chunk[..want];
            body.read_exact(buf).map_err(|e| match e.kind() {
                io::ErrorKind::UnexpectedEof => {
                    Error::Corrupt(format!("NCZ truncated: body ends before offset {i:#x}"))
                }
                _ => Error::Io(e),
            })?;
            if is_ctr(s.crypto_type) {
                ctr::keystream_xor(&s.crypto_key, &s.crypto_counter, i as u64, buf);
            }
            out.write_all(buf)?;
            hasher.update(&*buf);
            written += want as u64;
            i += want as i64;
        }
    }
    Ok(written)
}

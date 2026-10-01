//! Reading AVI files the way chdman does, a port of the reading side of
//! MAME's `src/lib/util/aviio.cpp`.
//!
//! Only the first `RIFF AVI ` is walked: OpenDML files reach their `AVIX`
//! extensions through the `indx` super index of each stream, and chunks
//! listed both there and in `idx1` are listed twice, MAME taking the first.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::error::{Error, Result};

const fn fourcc(code: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*code)
}

const CHUNKTYPE_RIFF: u32 = fourcc(b"RIFF");
const CHUNKTYPE_LIST: u32 = fourcc(b"LIST");
const CHUNKTYPE_AVIH: u32 = fourcc(b"avih");
const CHUNKTYPE_STRH: u32 = fourcc(b"strh");
const CHUNKTYPE_STRF: u32 = fourcc(b"strf");
const CHUNKTYPE_IDX1: u32 = fourcc(b"idx1");
const CHUNKTYPE_INDX: u32 = fourcc(b"indx");
const LISTTYPE_AVI: u32 = fourcc(b"AVI ");
const LISTTYPE_HDRL: u32 = fourcc(b"hdrl");
const LISTTYPE_STRL: u32 = fourcc(b"strl");
const LISTTYPE_MOVI: u32 = fourcc(b"movi");
pub(crate) const STREAMTYPE_VIDS: u32 = fourcc(b"vids");
pub(crate) const STREAMTYPE_AUDS: u32 = fourcc(b"auds");
const AVI_INDEX_OF_INDEXES: u8 = 0x00;
const AVI_INDEX_OF_CHUNKS: u8 = 0x01;

pub(crate) const FORMAT_UYVY: u32 = fourcc(b"UYVY");
pub(crate) const FORMAT_VYUY: u32 = fourcc(b"VYUY");
pub(crate) const FORMAT_YUY2: u32 = fourcc(b"YUY2");
pub(crate) const FORMAT_HFYU: u32 = fourcc(b"HFYU");

fn le16(data: &[u8], offset: usize) -> u32 {
    u32::from(u16::from_le_bytes([data[offset], data[offset + 1]]))
}

fn le32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn le64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}

fn invalid(what: &str) -> Error {
    Error::Corrupt(format!("invalid AVI: {what}"))
}

/// A chunk of the file: where its header is, its size and types.
#[derive(Clone, Copy, Default)]
struct Chunk {
    offset: u64,
    size: u64,
    kind: u32,
    listtype: u32,
}

/// One stream's headers and where its chunks are.
#[derive(Clone, Default)]
pub(crate) struct Stream {
    pub kind: u32,
    pub format: u32,
    pub rate: u32,
    pub scale: u32,
    pub samples: u32,
    /// Each chunk's header offset and length, header included.
    chunks: Vec<(u64, u32)>,
    pub width: u32,
    pub height: u32,
    pub channels: u32,
    pub samplebits: u32,
    pub samplerate: u32,
    /// The decoding tables of a HuffYUV video stream.
    huffyuv: Option<Box<[HuffYuvTable; 3]>>,
}

/// One HuffYUV Huffman table: every code's length, bits and mask, and the
/// lookups from the top 16 bits of the stream, `(value << 8) | length`, or
/// for longer codes a 0 length pointing at an extra 64 Ki table.
#[derive(Clone)]
struct HuffYuvTable {
    shift: [u8; 256],
    bits: [u32; 256],
    baselookup: Vec<u16>,
    extralookup: Vec<u16>,
}

/// `huffyuv_extract_tables`: the three tables (Y, Cb, Cr) of a HuffYUV
/// `strf`, run-length coded code lengths, left-predicted 16-bit YUV only.
fn huffyuv_extract_tables(chunk: &[u8]) -> Result<Box<[HuffYuvTable; 3]>> {
    if chunk.len() <= 41 {
        return Err(invalid("short HuffYUV header"));
    }
    if chunk[40] & !0x40 != 0 {
        return Err(Error::Unsupported(
            "HuffYUV predictor other than left".to_owned(),
        ));
    }
    if chunk[41] != 16 {
        return Err(Error::Unsupported(
            "HuffYUV other than 16-bit YUV".to_owned(),
        ));
    }
    let mut data = chunk.get(44..).unwrap_or_default().iter().copied();
    let empty = HuffYuvTable {
        shift: [0; 256],
        bits: [0; 256],
        baselookup: vec![0; 65536],
        extralookup: Vec::new(),
    };
    let mut tables = Box::new([empty.clone(), empty.clone(), empty]);
    for table in tables.iter_mut() {
        let mut offset = 0usize;
        while offset < 256 {
            let byte = data.next().ok_or_else(|| invalid("short HuffYUV tables"))?;
            let shift = byte & 0x1f;
            let mut count = usize::from(byte >> 5);
            if count == 0 {
                count = usize::from(data.next().ok_or_else(|| invalid("short HuffYUV tables"))?);
            }
            for _ in 0..count {
                // MAME writes on past the table; nothing is read from there
                if offset < 256 {
                    table.shift[offset] = shift;
                }
                offset += 1;
            }
        }

        // the canonical codes, longest first, and where the 17-bit ones end
        let mut curbits = 0u32;
        let mut bitsat16 = 0u16;
        for bits in (0..=31u32).rev() {
            // shifting by 32 is masked to 0, as on x86
            let bitadd = 1u32.wrapping_shl(32 - bits);
            if curbits & bitadd.wrapping_sub(1) != 0 {
                return Err(invalid("bad HuffYUV table"));
            }
            for offset in 0..256 {
                if u32::from(table.shift[offset]) == bits {
                    table.bits[offset] = curbits;
                    curbits = curbits.wrapping_add(bitadd);
                }
            }
            if bits == 17 {
                bitsat16 = (curbits >> 16) as u16;
            }
        }

        if bitsat16 > 0 {
            table.extralookup = vec![0; usize::from(bitsat16) * 65536];
            for offset in 0..usize::from(bitsat16) {
                table.baselookup[offset] = (offset << 8) as u16;
            }
        }
        for offset in 0..256 {
            let shift = u32::from(table.shift[offset]);
            if shift > 16 {
                let base = (table.bits[offset] >> 16) as usize * 65536;
                let start = (table.bits[offset] & 0xffff) as usize;
                let end = start + ((1usize << (32 - shift)) - 1);
                let lookup = table
                    .extralookup
                    .get_mut(base + start..=base + end)
                    .ok_or_else(|| invalid("bad HuffYUV table"))?;
                lookup.fill(((offset << 8) as u32 | (shift - 16)) as u16);
            } else if shift > 0 {
                let start = (table.bits[offset] >> 16) as usize;
                let end = start + ((1usize << (16 - shift)) - 1);
                let lookup = table
                    .baselookup
                    .get_mut(start..=end)
                    .ok_or_else(|| invalid("bad HuffYUV table"))?;
                lookup.fill(((offset << 8) as u32 | shift) as u16);
            }
        }
    }
    Ok(tables)
}

/// `huffyuv_decompress_to_yuy16`, left-predicted: the stream is read as
/// little-endian dwords, the first pixel pair stored as is.
fn huffyuv_decompress(
    tables: &[HuffYuvTable; 3],
    data: &[u8],
    numbytes: usize,
    width: usize,
    height: usize,
    bitmap: &mut [u16],
    rowpixels: usize,
) {
    // past the chunk, MAME reads on through its buffer, as this does; real
    // streams end on a whole dword, where it never gets to
    let byte = |index: usize| u32::from(data.get(index).copied().unwrap_or(0));
    let (mut lasty, mut lastcb, mut lastcr) = (0u8, 0u8, 0u8);
    let mut bitsinbuffer = 0u8;
    let mut bitbuffer = 0u32;
    let mut dataoffs = 0usize;
    let mut fill = |bitbuffer: &mut u32, bitsinbuffer: &mut u8, dataoffs: &mut usize| {
        while *bitsinbuffer <= 24 && *dataoffs < numbytes {
            *bitbuffer |= byte(*dataoffs ^ 3) << (24 - *bitsinbuffer);
            *dataoffs += 1;
            *bitsinbuffer += 8;
        }
    };
    let lookup = |table: &HuffYuvTable,
                  bitbuffer: &mut u32,
                  bitsinbuffer: &mut u8,
                  dataoffs: &mut usize,
                  fill: &mut dyn FnMut(&mut u32, &mut u8, &mut usize)| {
        let mut huffdata = table.baselookup[(*bitbuffer >> 16) as usize];
        let mut shift = u32::from(huffdata & 0xff);
        if shift == 0 {
            *bitsinbuffer = bitsinbuffer.wrapping_sub(16);
            *bitbuffer <<= 16;
            fill(bitbuffer, bitsinbuffer, dataoffs);
            huffdata = table
                .extralookup
                .get(usize::from(huffdata >> 8) * 65536 + (*bitbuffer >> 16) as usize)
                .copied()
                .unwrap_or(0);
            shift = u32::from(huffdata & 0xff);
        }
        *bitsinbuffer = bitsinbuffer.wrapping_sub(shift as u8);
        *bitbuffer = bitbuffer.checked_shl(shift).unwrap_or(0);
        huffdata
    };

    for y in 0..height {
        let row = &mut bitmap[y * rowpixels..y * rowpixels + width];
        let mut x = 0;
        if y == 0 {
            lasty = byte(dataoffs) as u8;
            lastcb = byte(dataoffs + 1) as u8;
            row[0] = (u16::from(lasty) << 8) | u16::from(lastcb);
            lasty = byte(dataoffs + 2) as u8;
            lastcr = byte(dataoffs + 3) as u8;
            row[1] = (u16::from(lasty) << 8) | u16::from(lastcr);
            dataoffs += 4;
            x = 2;
        }
        while x < width {
            fill(&mut bitbuffer, &mut bitsinbuffer, &mut dataoffs);
            let luma = lookup(
                &tables[0],
                &mut bitbuffer,
                &mut bitsinbuffer,
                &mut dataoffs,
                &mut fill,
            );
            fill(&mut bitbuffer, &mut bitsinbuffer, &mut dataoffs);
            let chroma = lookup(
                &tables[1 + (x & 1)],
                &mut bitbuffer,
                &mut bitsinbuffer,
                &mut dataoffs,
                &mut fill,
            );
            row[x] = (luma & 0xff00) | (chroma >> 8);
            x += 1;
        }
    }

    // left deltas, carried from row to row
    for y in 0..height {
        let row = &mut bitmap[y * rowpixels..y * rowpixels + width];
        let mut x = if y == 0 { 2 } else { 0 };
        while x < width {
            let pixel0 = row[x];
            lasty = lasty.wrapping_add((pixel0 >> 8) as u8);
            lastcb = lastcb.wrapping_add(pixel0 as u8);
            row[x] = (u16::from(lasty) << 8) | u16::from(lastcb);
            if x + 1 < width {
                let pixel1 = row[x + 1];
                lasty = lasty.wrapping_add((pixel1 >> 8) as u8);
                lastcr = lastcr.wrapping_add(pixel1 as u8);
                row[x + 1] = (u16::from(lasty) << 8) | u16::from(lastcr);
            }
            x += 2;
        }
    }
}

impl Stream {
    fn bytes_per_sample(&self) -> u32 {
        (self.samplebits / 8) * self.channels
    }

    /// `set_chunk_info`, which may only ever append here.
    fn push_chunk(&mut self, offset: u64, length: u32) {
        self.chunks.push((offset, length));
    }
}

/// The movie as a whole, MAME's `movie_info`.
#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct MovieInfo {
    pub video_format: u32,
    pub video_timescale: u32,
    pub video_sampletime: u32,
    pub video_numsamples: u32,
    pub video_width: u32,
    pub video_height: u32,
    pub audio_channels: u32,
    pub audio_samplerate: u32,
}

/// An AVI file open for reading.
pub(crate) struct AviReader {
    file: File,
    length: u64,
    streams: Vec<Stream>,
    pub info: MovieInfo,
    temp: Vec<u8>,
}

impl AviReader {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let length = file.metadata()?.len();
        let mut reader = Self {
            file,
            length,
            streams: Vec::new(),
            info: MovieInfo::default(),
            temp: Vec::new(),
        };
        reader.read_movie_data()?;
        Ok(reader)
    }

    pub fn size(&self) -> u64 {
        self.length
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.file.seek(SeekFrom::Start(offset))?;
        self.file
            .read_exact(buf)
            .map_err(|_| invalid("truncated chunk"))
    }

    fn root(&self) -> Chunk {
        Chunk {
            offset: 0,
            size: self.length,
            kind: 0,
            listtype: 0,
        }
    }

    /// The chunk at `offset` within `parent`, `None` past its end.
    fn chunk_at(&mut self, parent: &Chunk, offset: u64) -> Result<Option<Chunk>> {
        if offset + 8 >= parent.offset + 8 + parent.size {
            return Ok(None);
        }
        let mut header = [0u8; 8];
        self.read_at(offset, &mut header)?;
        let kind = le32(&header, 0);
        let mut chunk = Chunk {
            offset,
            size: u64::from(le32(&header, 4)),
            kind,
            listtype: 0,
        };
        if kind == CHUNKTYPE_LIST || kind == CHUNKTYPE_RIFF {
            let mut listtype = [0u8; 4];
            self.read_at(offset + 8, &mut listtype)?;
            chunk.listtype = le32(&listtype, 0);
        }
        Ok(Some(chunk))
    }

    /// Every chunk of `parent`, in order.
    fn children(&mut self, parent: &Chunk) -> Result<Vec<Chunk>> {
        if parent.kind != 0 && parent.kind != CHUNKTYPE_LIST && parent.kind != CHUNKTYPE_RIFF {
            return Err(invalid("not a list"));
        }
        let mut offset = if parent.kind != 0 {
            parent.offset + 12
        } else {
            0
        };
        let mut children = Vec::new();
        while let Some(chunk) = self.chunk_at(parent, offset)? {
            offset = chunk.offset + 8 + chunk.size + (chunk.size & 1);
            children.push(chunk);
        }
        Ok(children)
    }

    fn find(&mut self, parent: &Chunk, kind: u32) -> Result<Option<Chunk>> {
        Ok(self
            .children(parent)?
            .into_iter()
            .find(|chunk| chunk.kind == kind))
    }

    fn lists(&mut self, parent: &Chunk, listtype: u32) -> Result<Vec<Chunk>> {
        Ok(self
            .children(parent)?
            .into_iter()
            .filter(|chunk| chunk.kind == CHUNKTYPE_LIST && chunk.listtype == listtype)
            .collect())
    }

    fn chunk_data(&mut self, chunk: &Chunk) -> Result<Vec<u8>> {
        let mut data = vec![0u8; chunk.size as usize];
        self.read_at(chunk.offset + 8, &mut data)?;
        Ok(data)
    }

    /// `read_movie_data`.
    fn read_movie_data(&mut self) -> Result<()> {
        let root = self.root();
        let riff = self
            .find(&root, CHUNKTYPE_RIFF)?
            .ok_or_else(|| invalid("no RIFF chunk"))?;
        if riff.listtype != LISTTYPE_AVI {
            return Err(invalid("not an AVI"));
        }
        let hdrl = self
            .lists(&riff, LISTTYPE_HDRL)?
            .into_iter()
            .next()
            .ok_or_else(|| invalid("no hdrl list"))?;
        let avih = self
            .find(&hdrl, CHUNKTYPE_AVIH)?
            .ok_or_else(|| invalid("no avih chunk"))?;
        let avih = self.chunk_data(&avih)?;
        self.streams = vec![Stream::default(); le32(&avih, 24) as usize];

        for (index, strl) in self.lists(&hdrl, LISTTYPE_STRL)?.into_iter().enumerate() {
            if index >= self.streams.len() {
                // MAME stops there, with what it has
                break;
            }
            let strh = self
                .find(&strl, CHUNKTYPE_STRH)?
                .ok_or_else(|| invalid("no strh chunk"))?;
            let strh = self.chunk_data(&strh)?;
            let stream = &mut self.streams[index];
            stream.kind = le32(&strh, 0);
            stream.scale = le32(&strh, 20);
            stream.rate = le32(&strh, 24);
            stream.samples = le32(&strh, 32);

            let strf = self
                .find(&strl, CHUNKTYPE_STRF)?
                .ok_or_else(|| invalid("no strf chunk"))?;
            let strf = self.chunk_data(&strf)?;
            let stream = &mut self.streams[index];
            if stream.kind == STREAMTYPE_VIDS {
                stream.width = le32(&strf, 4);
                stream.height = le32(&strf, 8);
                stream.format = le32(&strf, 16);
                if stream.format == FORMAT_HFYU && strf.len() >= 56 {
                    stream.huffyuv = Some(huffyuv_extract_tables(&strf)?);
                }
            } else if stream.kind == STREAMTYPE_AUDS {
                stream.channels = le16(&strf, 2);
                stream.samplebits = le16(&strf, 14);
                stream.samplerate = le32(&strf, 4);
            }

            if let Some(indx) = self.find(&strl, CHUNKTYPE_INDX)? {
                self.parse_indx(index, &indx)?;
            }
        }

        let movi = self
            .lists(&riff, LISTTYPE_MOVI)?
            .into_iter()
            .next()
            .ok_or_else(|| invalid("no movi list"))?;
        if let Some(idx1) = self.find(&riff, CHUNKTYPE_IDX1)? {
            self.parse_idx1(movi.offset + 8, &idx1)?;
        }
        self.extract_movie_info()
    }

    /// `parse_indx_chunk`: a super index of standard indexes, or one of
    /// them, whose offsets point past each chunk's header.
    fn parse_indx(&mut self, stream: usize, indx: &Chunk) -> Result<()> {
        let data = self.chunk_data(indx)?;
        if data.len() < 24 {
            return Err(invalid("short index"));
        }
        let longs_per_entry = le16(&data, 0) as usize;
        let kind = data[3];
        let entries = le32(&data, 4) as usize;
        let baseoffset = le64(&data, 12);
        if kind == AVI_INDEX_OF_INDEXES {
            if longs_per_entry != 4 {
                return Err(invalid("bad super index"));
            }
            for entry in 0..entries {
                let base = data
                    .get(24 + entry * 16..24 + entry * 16 + 8)
                    .ok_or_else(|| invalid("short index"))?;
                let offset = le64(base, 0);
                let mut header = [0u8; 8];
                self.read_at(offset, &mut header)?;
                let sub = Chunk {
                    offset,
                    size: u64::from(le32(&header, 4)),
                    kind: le32(&header, 0),
                    listtype: 0,
                };
                self.parse_indx(stream, &sub)?;
            }
        } else if kind == AVI_INDEX_OF_CHUNKS {
            if longs_per_entry != 2 && longs_per_entry != 3 {
                return Err(invalid("bad standard index"));
            }
            for entry in 0..entries {
                let base = data
                    .get(24 + entry * 4 * longs_per_entry..)
                    .filter(|base| base.len() >= 8)
                    .ok_or_else(|| invalid("short index"))?;
                let offset = u64::from(le32(base, 0));
                // bit 31: not a keyframe
                let size = le32(base, 4) & 0x7fff_ffff;
                self.streams[stream].push_chunk(
                    baseoffset.wrapping_add(offset).wrapping_sub(8),
                    size.wrapping_add(8),
                );
            }
        }
        Ok(())
    }

    /// `parse_idx1_chunk`: offsets from the `movi` list's type.
    fn parse_idx1(&mut self, baseoffset: u64, idx1: &Chunk) -> Result<()> {
        let data = self.chunk_data(idx1)?;
        for entry in data.as_chunks::<16>().0 {
            let chunkid = le32(entry, 0);
            let streamnum = (((chunkid >> 8) & 0xff) as i64 - i64::from(b'0'))
                + 10 * ((chunkid & 0xff) as i64 - i64::from(b'0'));
            let stream = usize::try_from(streamnum)
                .ok()
                .filter(|&stream| stream < self.streams.len())
                .ok_or_else(|| invalid("index names an unknown stream"))?;
            self.streams[stream].push_chunk(
                baseoffset + u64::from(le32(entry, 8)),
                le32(entry, 12).wrapping_add(8),
            );
        }
        Ok(())
    }

    fn video_stream(&self) -> Option<usize> {
        self.streams
            .iter()
            .position(|stream| stream.kind == STREAMTYPE_VIDS)
    }

    /// The stream carrying audio channel `channel`, and the channel's
    /// position in it.
    fn audio_stream(&self, mut channel: u32) -> Option<(usize, u32)> {
        for (index, stream) in self.streams.iter().enumerate() {
            if stream.kind == STREAMTYPE_AUDS {
                if channel < stream.channels {
                    return Some((index, channel));
                }
                channel -= stream.channels;
            }
        }
        None
    }

    /// `extract_movie_info`.
    fn extract_movie_info(&mut self) -> Result<()> {
        if let Some(video) = self.video_stream() {
            let stream = &self.streams[video];
            self.info.video_format = stream.format;
            self.info.video_timescale = stream.rate;
            self.info.video_sampletime = stream.scale;
            self.info.video_numsamples = stream.samples;
            self.info.video_width = stream.width;
            self.info.video_height = stream.height;
        }
        if let Some((first, _)) = self.audio_stream(0) {
            let first = self.streams[first].clone();
            self.info.audio_channels = 1;
            self.info.audio_samplerate = first.samplerate;
            while let Some((index, _)) = self.audio_stream(self.info.audio_channels) {
                self.info.audio_channels += 1;
                let stream = &self.streams[index];
                if stream.format != first.format
                    || stream.rate != first.rate
                    || stream.scale != first.scale
                    || stream.samples != first.samples
                    || stream.samplebits != first.samplebits
                    || stream.samplerate != first.samplerate
                {
                    return Err(Error::Unsupported(
                        "incompatible AVI audio streams".to_owned(),
                    ));
                }
            }
        }
        Ok(())
    }

    /// The chunk id MAME expects for stream `index`.
    fn chunkid(&self, index: usize) -> u32 {
        let stream = &self.streams[index];
        let base = u32::from(b'0' + (index / 10) as u8) | u32::from(b'0' + (index % 10) as u8) << 8;
        base | if stream.kind == STREAMTYPE_VIDS {
            if stream.format == 0 {
                fourcc(b"\0\0db")
            } else {
                fourcc(b"\0\0dc")
            }
        } else if stream.kind == STREAMTYPE_AUDS {
            fourcc(b"\0\0wb")
        } else {
            0
        }
    }

    /// Reads chunk `chunknum` of stream `index` into the temporary buffer,
    /// checking its id.
    fn read_chunk(&mut self, index: usize, chunknum: usize) -> Result<usize> {
        let (offset, length) = self.streams[index].chunks[chunknum];
        let length = length as usize;
        if self.temp.len() < length {
            self.temp.resize(length * 2, 0);
        }
        let mut temp = std::mem::take(&mut self.temp);
        let result = self.read_at(offset, &mut temp[..length]);
        self.temp = temp;
        result?;
        if length < 8 || le32(&self.temp, 0) != self.chunkid(index) {
            return Err(invalid("chunk of the wrong stream"));
        }
        Ok(length)
    }

    /// `read_video_frame`: frame `framenum` into `bitmap`, rows of
    /// `rowpixels` YUY16 pixels (luma in the high byte). What the frame
    /// does not cover is left as it was.
    pub fn read_video_frame(
        &mut self,
        framenum: u32,
        bitmap: &mut [u16],
        rowpixels: usize,
    ) -> Result<()> {
        let index = self
            .video_stream()
            .ok_or_else(|| invalid("no video stream"))?;
        let stream = &self.streams[index];
        let (format, width, height) =
            (stream.format, stream.width as usize, stream.height as usize);
        match format {
            FORMAT_UYVY | FORMAT_VYUY | FORMAT_YUY2 | FORMAT_HFYU => {}
            _ => return Err(Error::Unsupported("AVI video format".to_owned())),
        }
        if framenum as usize >= stream.chunks.len() {
            return Err(invalid("frame out of range"));
        }
        let length = self.read_chunk(index, framenum as usize)?;
        if format == FORMAT_HFYU {
            let tables = self.streams[index]
                .huffyuv
                .as_ref()
                .ok_or_else(|| invalid("HuffYUV stream without tables"))?;
            huffyuv_decompress(
                tables,
                &self.temp[8..],
                length - 8,
                width,
                height,
                bitmap,
                rowpixels,
            );
            return Ok(());
        }
        let data = &self.temp[8..length];
        let values = data.len() / 2;
        for y in 0..height {
            let row = &mut bitmap[y * rowpixels..];
            for (x, pixel) in row.iter_mut().enumerate().take(width) {
                let source = y * width + x;
                if source >= values {
                    break;
                }
                let raw = u16::from_le_bytes([data[source * 2], data[source * 2 + 1]]);
                *pixel = if format == FORMAT_UYVY {
                    raw
                } else {
                    raw.swap_bytes()
                };
            }
        }
        Ok(())
    }

    /// `read_sound_samples`: `out.len()` samples of `channel` from
    /// `firstsample`, as many as the stream has; past its last chunk,
    /// silence.
    pub fn read_sound_samples(
        &mut self,
        channel: u32,
        mut firstsample: u32,
        out: &mut [i16],
    ) -> Result<()> {
        let (index, offset) = self
            .audio_stream(channel)
            .ok_or_else(|| invalid("no such audio channel"))?;
        let stream = &self.streams[index];
        if stream.format != 0 || (stream.samplebits != 8 && stream.samplebits != 16) {
            return Err(Error::Unsupported("AVI audio format".to_owned()));
        }
        if firstsample >= stream.samples {
            return Err(invalid("sample out of range"));
        }
        let mut numsamples = (out.len() as u32).min(stream.samples - firstsample) as usize;
        let bytes_per_sample = stream.bytes_per_sample();
        if bytes_per_sample == 0 {
            return Err(invalid("audio stream without samples"));
        }
        let channels = stream.channels as usize;
        let samplebits = stream.samplebits;
        let mut written = 0;
        while numsamples > 0 {
            let stream = &self.streams[index];
            let mut chunkbase = 0u32;
            let mut chunkend = 0u32;
            let mut found = None;
            for (chunknum, &(_, length)) in stream.chunks.iter().enumerate() {
                chunkend = chunkbase.wrapping_add(length.wrapping_sub(8) / bytes_per_sample);
                if firstsample < chunkend {
                    found = Some(chunknum);
                    break;
                }
                chunkbase = chunkend;
            }
            let Some(chunknum) = found else {
                out[written..written + numsamples].fill(0);
                break;
            };
            self.read_chunk(index, chunknum)?;
            let count = ((chunkend - firstsample) as usize).min(numsamples);
            let data = &self.temp[8..];
            let first = channels * (firstsample - chunkbase) as usize + offset as usize;
            for sample in 0..count {
                let position = first + sample * channels;
                out[written + sample] = if samplebits == 16 {
                    i16::from_le_bytes([data[position * 2], data[position * 2 + 1]])
                } else {
                    ((i32::from(data[position]) << 8) - 0x8000) as i16
                };
            }
            written += count;
            firstsample += count as u32;
            numsamples -= count;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Writing: the creating side of `aviio.cpp`, as `chdman extractld` drives
// it. Chunks are written at explicit offsets: sound chunks are reserved
// ahead of the video and filled in once their samples arrive, headers are
// rewritten at the end, and the pad byte of an odd-sized chunk is skipped
// over rather than written.
// ---------------------------------------------------------------------------

use std::io::Write;

const CHUNKTYPE_JUNK: u32 = fourcc(b"JUNK");
const LISTTYPE_AVIX: u32 = fourcc(b"AVIX");
const HANDLER_DIB: u32 = fourcc(b"DIB ");
const HANDLER_HFYU: u32 = fourcc(b"hfyu");
const AVIF_HASINDEX: u32 = 0x10;
const AVIF_ISINTERLEAVED: u32 = 0x100;
/// Just under 2 GiB, where a new `RIFF AVIX` begins.
const MAX_RIFF_SIZE: u64 = 2 * 1024 * 1024 * 1024 - 1024;
const MAX_AVI_SIZE_IN_GB: usize = 1024;
const FOUR_GB: u64 = 1 << 32;
const MAX_SOUND_CHANNELS: u32 = 16;
const SOUND_BUFFER_MSEC: u32 = 2000;
const AVI_INTEGRAL_MULTIPLE: u32 = 4;
/// The size of an `indx` chunk's data, super index entries included.
const INDX_SIZE: usize = 24 + 16 * MAX_AVI_SIZE_IN_GB / 4;

/// What a new AVI holds: YUY2 video, 16-bit PCM audio.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CreateInfo {
    pub video_timescale: u32,
    pub video_sampletime: u32,
    pub video_width: u32,
    pub video_height: u32,
    pub audio_channels: u32,
    pub audio_samplerate: u32,
}

#[derive(Clone, Copy)]
struct OpenChunk {
    offset: u64,
    size: u64,
}

#[derive(Default)]
struct WriteStream {
    stream: Stream,
    depth: u32,
    saved_strh_offset: u64,
    saved_indx_offset: u64,
}

/// An AVI being written.
pub(crate) struct AviWriter {
    file: File,
    info: CreateInfo,
    streams: Vec<WriteStream>,
    writeoffs: u64,
    riffbase: u64,
    chunkstack: Vec<OpenChunk>,
    saved_movi_offset: u64,
    saved_avih_offset: u64,
    soundbuf: Vec<i16>,
    soundbuf_samples: u32,
    chansamples: [u32; MAX_SOUND_CHANNELS as usize],
    soundbuf_chunks: u32,
    soundbuf_frames: u32,
    temp: Vec<u8>,
}

impl AviWriter {
    /// `avi_file::create`: validates the format, sets up the sound buffer
    /// and writes the initial headers.
    pub fn create(path: &Path, info: CreateInfo) -> Result<Self> {
        if info.video_width == 0 || info.video_height == 0 {
            return Err(Error::Unsupported("AVI video format".to_owned()));
        }
        if info.audio_channels > MAX_SOUND_CHANNELS {
            return Err(Error::Unsupported("AVI audio format".to_owned()));
        }
        let file = File::create(path)?;
        let mut video = WriteStream {
            depth: 16,
            ..WriteStream::default()
        };
        video.stream.kind = STREAMTYPE_VIDS;
        video.stream.format = FORMAT_YUY2;
        video.stream.rate = info.video_timescale;
        video.stream.scale = info.video_sampletime;
        video.stream.width = info.video_width - info.video_width % AVI_INTEGRAL_MULTIPLE;
        video.stream.height = info.video_height - info.video_height % AVI_INTEGRAL_MULTIPLE;
        let mut streams = vec![video];
        if info.audio_channels > 0 {
            let mut audio = WriteStream::default();
            audio.stream.kind = STREAMTYPE_AUDS;
            audio.stream.rate = info.audio_samplerate;
            audio.stream.scale = 1;
            audio.stream.channels = info.audio_channels;
            audio.stream.samplebits = 16;
            audio.stream.samplerate = info.audio_samplerate;
            streams.push(audio);
        }
        let mut writer = Self {
            file,
            info,
            streams,
            writeoffs: 0,
            riffbase: 0,
            chunkstack: Vec::new(),
            saved_movi_offset: 0,
            saved_avih_offset: 0,
            soundbuf: Vec::new(),
            soundbuf_samples: 0,
            chansamples: [0; MAX_SOUND_CHANNELS as usize],
            soundbuf_chunks: 0,
            soundbuf_frames: 0,
            temp: Vec::new(),
        };
        writer.soundbuf_initialize();
        writer.write_initial_headers()?;
        Ok(writer)
    }

    fn audio(&self) -> Option<usize> {
        (self.streams.len() > 1).then_some(1)
    }

    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(data)?;
        Ok(())
    }

    fn chunkid(&self, index: usize) -> u32 {
        let stream = &self.streams[index].stream;
        let base = u32::from(b'0' + (index / 10) as u8) | u32::from(b'0' + (index % 10) as u8) << 8;
        base | if stream.kind == STREAMTYPE_VIDS {
            if stream.format == 0 {
                fourcc(b"\0\0db")
            } else {
                fourcc(b"\0\0dc")
            }
        } else {
            fourcc(b"\0\0wb")
        }
    }

    fn compute_idx1_size(&self) -> u32 {
        let chunks: usize = self
            .streams
            .iter()
            .map(|stream| stream.stream.chunks.len())
            .sum();
        (chunks * 16 + 8) as u32
    }

    fn framenum_to_samplenum(&self, framenum: u32) -> u32 {
        (u64::from(self.info.audio_samplerate)
            * u64::from(framenum)
            * u64::from(self.info.video_sampletime))
        .div_ceil(u64::from(self.info.video_timescale)) as u32
    }

    fn chunk_open(&mut self, kind: u32, listtype: Option<u32>, estlength: u32) -> Result<()> {
        if self.chunkstack.len() >= 8 {
            return Err(Error::Corrupt("AVI chunks nest too deep".to_owned()));
        }
        self.chunkstack.push(OpenChunk {
            offset: self.writeoffs,
            size: u64::from(estlength),
        });
        let mut header = Vec::with_capacity(12);
        header.extend_from_slice(&kind.to_le_bytes());
        header.extend_from_slice(&estlength.to_le_bytes());
        if let Some(listtype) = listtype {
            header.extend_from_slice(&listtype.to_le_bytes());
        }
        self.write_at(self.writeoffs, &header)?;
        self.writeoffs += header.len() as u64;
        Ok(())
    }

    fn chunk_close(&mut self) -> Result<()> {
        let chunk = self.chunkstack.pop().expect("an open AVI chunk");
        let chunksize = self.writeoffs - (chunk.offset + 8);
        let Ok(size) = u32::try_from(chunksize) else {
            return Err(Error::Corrupt("AVI chunk over 4 GiB".to_owned()));
        };
        if chunk.size != chunksize {
            self.write_at(chunk.offset + 4, &size.to_le_bytes())?;
        }
        // the pad byte is skipped, not written
        self.writeoffs += chunksize & 1;
        Ok(())
    }

    fn chunk_write(&mut self, kind: u32, data: &[u8]) -> Result<()> {
        let length = data.len() as u64;
        let idxreserve = if self.riffbase == 0 && kind != CHUNKTYPE_IDX1 {
            u64::from(self.compute_idx1_size())
        } else {
            0
        };
        // past 2 GiB, the movie continues in a new RIFF; writes before the
        // current one are overwrites of chunks of a previous one
        if self.writeoffs >= self.riffbase
            && self.writeoffs + length + idxreserve - self.riffbase >= MAX_RIFF_SIZE
        {
            self.chunk_close()?;
            if self.riffbase == 0 {
                self.write_idx1_chunk()?;
            }
            self.chunk_close()?;
            self.riffbase = self.writeoffs;
            self.chunk_open(CHUNKTYPE_RIFF, Some(LISTTYPE_AVIX), 0)?;
            self.saved_movi_offset = self.writeoffs;
            self.chunk_open(CHUNKTYPE_LIST, Some(LISTTYPE_MOVI), 0)?;
        }
        self.chunk_open(kind, None, data.len() as u32)?;
        self.write_at(self.writeoffs, data)?;
        self.writeoffs += length;
        self.chunk_close()
    }

    /// Writes a chunk at the end the first time, saving where, and over
    /// that place afterwards.
    fn chunk_overwrite(
        &mut self,
        kind: u32,
        data: &[u8],
        offset: &mut u64,
        initial: bool,
    ) -> Result<()> {
        if initial {
            *offset = self.writeoffs;
            return self.chunk_write(kind, data);
        }
        let saved = self.writeoffs;
        self.writeoffs = *offset;
        let result = self.chunk_write(kind, data);
        self.writeoffs = saved;
        result
    }

    fn write_initial_headers(&mut self) -> Result<()> {
        self.writeoffs = 0;
        self.chunk_open(CHUNKTYPE_RIFF, Some(LISTTYPE_AVI), 0)?;
        self.chunk_open(CHUNKTYPE_LIST, Some(LISTTYPE_HDRL), 0)?;
        self.write_avih_chunk(true)?;
        for index in 0..self.streams.len() {
            self.chunk_open(CHUNKTYPE_LIST, Some(LISTTYPE_STRL), 0)?;
            self.write_strh_chunk(index, true)?;
            self.write_strf_chunk(index)?;
            self.write_indx_chunk(index, true)?;
            self.chunk_close()?;
        }
        self.chunk_close()?;
        self.saved_movi_offset = self.writeoffs;
        self.chunk_open(CHUNKTYPE_LIST, Some(LISTTYPE_MOVI), 0)
    }

    fn write_avih_chunk(&mut self, initial: bool) -> Result<()> {
        let video = &self.streams[0].stream;
        let mut buffer = [0u8; 56];
        let per_frame = (1_000_000 * i64::from(video.scale) / i64::from(video.rate)) as u32;
        buffer[0..4].copy_from_slice(&per_frame.to_le_bytes());
        buffer[12..16].copy_from_slice(&(AVIF_HASINDEX | AVIF_ISINTERLEAVED).to_le_bytes());
        buffer[16..20].copy_from_slice(&video.samples.to_le_bytes());
        buffer[24..28].copy_from_slice(&(self.streams.len() as u32).to_le_bytes());
        buffer[32..36].copy_from_slice(&video.width.to_le_bytes());
        buffer[36..40].copy_from_slice(&video.height.to_le_bytes());
        let mut offset = self.saved_avih_offset;
        self.chunk_overwrite(CHUNKTYPE_AVIH, &buffer, &mut offset, initial)?;
        self.saved_avih_offset = offset;
        Ok(())
    }

    fn write_strh_chunk(&mut self, index: usize, initial: bool) -> Result<()> {
        let stream = &self.streams[index].stream;
        let mut buffer = [0u8; 56];
        buffer[0..4].copy_from_slice(&stream.kind.to_le_bytes());
        buffer[20..24].copy_from_slice(&stream.scale.to_le_bytes());
        buffer[24..28].copy_from_slice(&stream.rate.to_le_bytes());
        buffer[32..36].copy_from_slice(&stream.samples.to_le_bytes());
        buffer[40..44].copy_from_slice(&10000u32.to_le_bytes());
        if stream.kind == STREAMTYPE_VIDS {
            let handler = if stream.format == FORMAT_HFYU {
                HANDLER_HFYU
            } else {
                HANDLER_DIB
            };
            buffer[4..8].copy_from_slice(&handler.to_le_bytes());
            buffer[36..40].copy_from_slice(&(stream.width * stream.height * 4).to_le_bytes());
            buffer[52..54].copy_from_slice(&(stream.width as u16).to_le_bytes());
            buffer[54..56].copy_from_slice(&(stream.height as u16).to_le_bytes());
        } else {
            let bytes_per_sample = stream.bytes_per_sample();
            buffer[36..40].copy_from_slice(&(stream.samplerate * bytes_per_sample).to_le_bytes());
            buffer[44..48].copy_from_slice(&bytes_per_sample.to_le_bytes());
        }
        let mut offset = self.streams[index].saved_strh_offset;
        self.chunk_overwrite(CHUNKTYPE_STRH, &buffer, &mut offset, initial)?;
        self.streams[index].saved_strh_offset = offset;
        Ok(())
    }

    fn write_strf_chunk(&mut self, index: usize) -> Result<()> {
        let WriteStream { stream, depth, .. } = &self.streams[index];
        let buffer = if stream.kind == STREAMTYPE_VIDS {
            let mut buffer = vec![0u8; 40];
            buffer[0..4].copy_from_slice(&40u32.to_le_bytes());
            buffer[4..8].copy_from_slice(&stream.width.to_le_bytes());
            buffer[8..12].copy_from_slice(&stream.height.to_le_bytes());
            buffer[12..14].copy_from_slice(&1u16.to_le_bytes());
            buffer[14..16].copy_from_slice(&(*depth as u16).to_le_bytes());
            buffer[16..20].copy_from_slice(&stream.format.to_le_bytes());
            buffer[20..24]
                .copy_from_slice(&(stream.width * stream.height * (depth + 7) / 8).to_le_bytes());
            buffer
        } else {
            let bytes_per_sample = stream.bytes_per_sample();
            let mut buffer = vec![0u8; 16];
            buffer[0..2].copy_from_slice(&1u16.to_le_bytes());
            buffer[2..4].copy_from_slice(&(stream.channels as u16).to_le_bytes());
            buffer[4..8].copy_from_slice(&stream.samplerate.to_le_bytes());
            buffer[8..12].copy_from_slice(&(stream.samplerate * bytes_per_sample).to_le_bytes());
            buffer[12..14].copy_from_slice(&(bytes_per_sample as u16).to_le_bytes());
            buffer[14..16].copy_from_slice(&(stream.samplebits as u16).to_le_bytes());
            buffer
        };
        self.chunk_write(CHUNKTYPE_STRF, &buffer)
    }

    /// The `indx` placeholder, `JUNK` until the movie outgrows one RIFF;
    /// then standard indexes for each 4 GiB of it and a super index of them.
    fn write_indx_chunk(&mut self, index: usize, initial: bool) -> Result<()> {
        let mut buffer = vec![0u8; INDX_SIZE];
        let mut master_entries = 0usize;
        let chunkid = self.chunkid(index);
        let indexchunkid = u32::from_le_bytes([
            b'i',
            b'x',
            b'0' + (index / 10) as u8,
            b'0' + (index % 10) as u8,
        ]);
        if !initial && self.riffbase != 0 {
            let mut currentbase = 0u64;
            while currentbase < self.writeoffs {
                let currentend = currentbase + FOUR_GB;
                let stream = &self.streams[index].stream;
                let chunks: Vec<(u64, u32)> = stream
                    .chunks
                    .iter()
                    .copied()
                    .filter(|&(offset, _)| offset >= currentbase && offset < currentend)
                    .collect();
                if chunks.is_empty() {
                    currentbase += FOUR_GB;
                    continue;
                }
                if master_entries >= MAX_AVI_SIZE_IN_GB / 4 {
                    return Err(Error::Corrupt("AVI too large".to_owned()));
                }
                let mut temp = vec![0u8; 24 + 8 * chunks.len()];
                temp[0..2].copy_from_slice(&2u16.to_le_bytes());
                temp[3] = AVI_INDEX_OF_CHUNKS;
                temp[4..8].copy_from_slice(&(chunks.len() as u32).to_le_bytes());
                temp[8..12].copy_from_slice(&chunkid.to_le_bytes());
                temp[12..20].copy_from_slice(&currentbase.to_le_bytes());
                let mut bytes = 0u32;
                for (entry, &(offset, length)) in chunks.iter().enumerate() {
                    temp[24 + 8 * entry..][..4]
                        .copy_from_slice(&((offset + 8 - currentbase) as u32).to_le_bytes());
                    temp[28 + 8 * entry..][..4].copy_from_slice(&(length - 8).to_le_bytes());
                    bytes = bytes.wrapping_add(length);
                }
                let base = 24 + 16 * master_entries;
                buffer[base..base + 8].copy_from_slice(&self.writeoffs.to_le_bytes());
                buffer[base + 8..base + 12]
                    .copy_from_slice(&((24 + 8 * chunks.len() + 8) as u32).to_le_bytes());
                let duration = if stream.kind == STREAMTYPE_VIDS {
                    chunks.len() as u32
                } else {
                    bytes / stream.bytes_per_sample()
                };
                buffer[base + 12..base + 16].copy_from_slice(&duration.to_le_bytes());
                master_entries += 1;
                self.chunk_write(indexchunkid, &temp)?;
                currentbase += FOUR_GB;
            }
        }
        if master_entries != 0 {
            buffer[0..2].copy_from_slice(&4u16.to_le_bytes());
            buffer[3] = AVI_INDEX_OF_INDEXES;
            buffer[4..8].copy_from_slice(&(master_entries as u32).to_le_bytes());
            buffer[8..12].copy_from_slice(&chunkid.to_le_bytes());
        }
        let kind = if master_entries == 0 {
            CHUNKTYPE_JUNK
        } else {
            CHUNKTYPE_INDX
        };
        let mut offset = self.streams[index].saved_indx_offset;
        self.chunk_overwrite(kind, &buffer, &mut offset, initial)?;
        self.streams[index].saved_indx_offset = offset;
        Ok(())
    }

    /// The `idx1` of the first RIFF: every chunk, in file order.
    fn write_idx1_chunk(&mut self) -> Result<()> {
        let length = self.compute_idx1_size() as usize - 8;
        let mut temp = vec![0u8; length];
        let mut curchunk = [0usize; 2];
        for entry in temp.as_chunks_mut::<16>().0 {
            let mut minoffset = u64::MAX;
            let mut minstr = 0;
            for (index, stream) in self.streams.iter().enumerate() {
                if let Some(&(offset, _)) = stream.stream.chunks.get(curchunk[index])
                    && offset < minoffset
                {
                    minoffset = offset;
                    minstr = index;
                }
            }
            let (_, length) = self.streams[minstr].stream.chunks[curchunk[minstr]];
            entry[0..4].copy_from_slice(&self.chunkid(minstr).to_le_bytes());
            entry[4..8].copy_from_slice(&0x10u32.to_le_bytes());
            entry[8..12].copy_from_slice(
                &(minoffset.wrapping_sub(self.saved_movi_offset + 8) as u32).to_le_bytes(),
            );
            entry[12..16].copy_from_slice(&(length - 8).to_le_bytes());
            curchunk[minstr] += 1;
        }
        self.chunk_write(CHUNKTYPE_IDX1, &temp)
    }

    fn soundbuf_initialize(&mut self) {
        let Some(_) = self.audio() else {
            return;
        };
        self.soundbuf_samples = self.info.audio_samplerate * SOUND_BUFFER_MSEC / 1000;
        self.soundbuf = vec![0; (self.soundbuf_samples * self.info.audio_channels) as usize];
        let video = &self.streams[0].stream;
        self.soundbuf_frames =
            ((u64::from(video.rate) * 75) / (u64::from(video.scale) * 100) + 1) as u32;
    }

    fn soundbuf_bytes(&self, length: usize) -> Vec<u8> {
        self.soundbuf[..length / 2]
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect()
    }

    /// Reserves the sound chunk that precedes video frame `framenum`,
    /// written with whatever the buffer holds for now.
    fn soundbuf_write_chunk(&mut self, framenum: u32) -> Result<()> {
        let Some(audio) = self.audio() else {
            return Ok(());
        };
        let samples = if framenum == 0 {
            self.framenum_to_samplenum(self.soundbuf_frames)
        } else {
            self.framenum_to_samplenum(framenum + 1 + self.soundbuf_frames)
                .wrapping_sub(self.framenum_to_samplenum(framenum + self.soundbuf_frames))
        };
        let length = samples as usize * self.streams[audio].stream.channels as usize * 2;
        let data = self.soundbuf_bytes(length);
        self.chunk_write(self.chunkid(audio), &data)?;
        let offset = self.writeoffs - length as u64 - 8;
        self.streams[audio]
            .stream
            .push_chunk(offset, length as u32 + 8);
        Ok(())
    }

    /// Fills in the reserved sound chunks the buffer has the samples for;
    /// at the end, all of them, padding with silence, and any it has
    /// nothing for become `JUNK` and leave the index.
    fn soundbuf_flush(&mut self, only_flush_full: bool) -> Result<()> {
        let Some(audio) = self.audio() else {
            return Ok(());
        };
        let channels = self.streams[audio].stream.channels as usize;
        let mut chunkid = self.chunkid(audio);
        let bytes_per_sample = channels as u32 * 2;
        let mut finalchunks = self.streams[audio].stream.chunks.len();
        let mut channelsamples = self.soundbuf_samples as i32;
        for channel in 0..channels {
            channelsamples = channelsamples.min(self.chansamples[channel] as i32);
        }
        let mut processedsamples: i32 = 0;
        let mut chunknum = self.soundbuf_chunks as usize;
        while chunknum < self.streams[audio].stream.chunks.len() {
            let (mut offset, length) = self.streams[audio].stream.chunks[chunknum];
            let chunksamples = (length - 8) / bytes_per_sample;
            if only_flush_full && (channelsamples as u32) < chunksamples {
                break;
            }
            if channelsamples > 0 && (channelsamples as u32) < chunksamples {
                if (processedsamples as u32).wrapping_add(chunksamples) > self.soundbuf_samples {
                    return Err(Error::Corrupt("AVI sound buffer overflow".to_owned()));
                }
                let from = (processedsamples + channelsamples) as usize * channels;
                let count = (chunksamples - channelsamples as u32) as usize * channels;
                self.soundbuf[from..from + count].fill(0);
            } else if channelsamples <= 0 {
                processedsamples = self.soundbuf_samples as i32 - chunksamples as i32;
                let from = processedsamples as usize * channels;
                let count = chunksamples as usize * channels;
                self.soundbuf[from..from + count].fill(0);
                chunkid = CHUNKTYPE_JUNK;
                finalchunks -= 1;
            }
            let from = processedsamples as usize * channels;
            let data: Vec<u8> = self.soundbuf[from..from + (length as usize - 8) / 2]
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect();
            self.chunk_overwrite(chunkid, &data, &mut offset, false)?;
            let stream = &mut self.streams[audio].stream;
            if channelsamples as u32 > chunksamples && channelsamples > 0 {
                stream.samples = stream.samples.wrapping_add(chunksamples);
            } else if channelsamples > 0 {
                stream.samples = stream.samples.wrapping_add(channelsamples as u32);
            }
            processedsamples += chunksamples as i32;
            channelsamples = (channelsamples - chunksamples as i32).max(0);
            chunknum += 1;
        }
        if processedsamples > 0 {
            let processed = processedsamples as u32;
            if self.soundbuf_samples > processed {
                let from = processed as usize * channels;
                let count = (self.soundbuf_samples - processed) as usize * channels;
                self.soundbuf.copy_within(from..from + count, 0);
            }
            for channel in 0..channels {
                self.chansamples[channel] = self.chansamples[channel].wrapping_sub(processed);
            }
        }
        if !only_flush_full {
            self.streams[audio].stream.chunks.truncate(finalchunks);
        }
        self.soundbuf_chunks = chunknum as u32;
        Ok(())
    }

    /// `append_sound_samples`: buffers a channel's samples, then writes
    /// the sound chunks that are complete.
    pub fn append_sound_samples(&mut self, channel: usize, samples: &[i16]) -> Result<()> {
        let channels = self.info.audio_channels as usize;
        let sampoffset = self.chansamples[channel];
        if sampoffset as usize + samples.len() > self.soundbuf_samples as usize {
            return Err(Error::Corrupt("AVI sound buffer overflow".to_owned()));
        }
        for (index, &sample) in samples.iter().enumerate() {
            self.soundbuf[(sampoffset as usize + index) * channels + channel] = sample;
        }
        self.chansamples[channel] = sampoffset + samples.len() as u32;
        self.soundbuf_flush(true)
    }

    /// `append_video_frame`: the frame's sound chunk, then the frame as
    /// YUY2 from a bitmap of `rowpixels` YUY16 pixels a row.
    pub fn append_video_frame(&mut self, bitmap: &[u16], rowpixels: usize) -> Result<()> {
        let frames = self.streams[0].stream.chunks.len() as u32;
        self.soundbuf_write_chunk(frames)?;
        let (width, height) = (
            self.streams[0].stream.width as usize,
            self.streams[0].stream.height as usize,
        );
        let maxlength = 2 * width * height;
        if self.temp.len() < maxlength {
            self.temp.resize(maxlength * 2, 0);
        }
        for y in 0..height {
            let row = &bitmap[y * rowpixels..][..width];
            for (x, pixel) in row.iter().enumerate() {
                self.temp[(y * width + x) * 2..][..2].copy_from_slice(&pixel.to_be_bytes());
            }
        }
        let data = std::mem::take(&mut self.temp);
        let result = self.chunk_write(self.chunkid(0), &data[..maxlength]);
        self.temp = data;
        result?;
        let offset = self.writeoffs - maxlength as u64 - 8;
        let video = &mut self.streams[0].stream;
        video.push_chunk(offset, maxlength as u32 + 8);
        video.samples = video.chunks.len() as u32;
        Ok(())
    }

    /// Finalizes the file, as MAME's destructor does: the last sound, the
    /// indexes and the headers.
    pub fn finish(mut self) -> Result<()> {
        self.soundbuf_flush(false)?;
        self.chunk_close()?;
        if self.riffbase == 0 {
            self.write_idx1_chunk()?;
        }
        for index in 0..self.streams.len() {
            self.write_strh_chunk(index, false)?;
            self.write_indx_chunk(index, false)?;
        }
        self.write_avih_chunk(false)?;
        self.chunk_close()?;
        self.file.flush()?;
        Ok(())
    }
}

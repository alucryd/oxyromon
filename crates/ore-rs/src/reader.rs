//! Reading an ORE as the disc it holds.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use rayon::prelude::*;

use super::codec;
use super::crypto::Key;
use super::error::{Error, Result};
use super::format::*;
use super::io;
use super::lfg::{Lfg, SEED_BYTES};
use super::segment::{BLOCK, RECORD_MAX};
use super::wii::{
    self, BLOCK_DATA_SIZE, BLOCK_TOTAL_SIZE, BLOCKS_PER_GROUP, BlockData, GROUP_DATA_SIZE,
    GROUP_TOTAL_SIZE, HashBlock,
};
use super::writer::Stats;

/// What a core decodes at a time when writing a disc out: four Wii groups.
const SLICE: u64 = 4 * GROUP_TOTAL_SIZE;

enum Record {
    Zero,
    Padding(Box<[u8; SEED_BYTES]>),
    Chunk(u32),
}

struct AreaIndex {
    start: u64,
    len: u64,
    kind: AreaKind,
    key: Option<Key>,
    exceptions: HashMap<u64, HashBlock>,
    /// Where each record starts in the area's stored stream, and where the
    /// last ends.
    starts: Vec<u64>,
    records: Vec<Record>,
}

/// Where a stored chunk lies.
#[derive(Clone, Copy)]
struct ChunkAt {
    group: u32,
    offset: u32,
}

struct GroupAt {
    /// Where its envelope starts.
    offset: u64,
    envelope_len: u64,
    group: Group,
}

/// An ORE, read as the disc it holds.
pub struct Reader {
    file: File,
    header: Header,
    meta: Meta,
    areas: Vec<AreaIndex>,
    chunks: Vec<ChunkAt>,
    groups: Vec<GroupAt>,
    cache: HashMap<u32, Arc<Vec<u8>>>,
    /// The Wii group last encrypted: its area, its number, its bytes.
    wii: Option<(usize, u64, Vec<u8>)>,
}

fn corrupt(what: &str) -> Error {
    Error::Corrupt(what.into())
}

impl Reader {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let file_size = file.metadata()?.len();
        let mut bytes = [0; HEADER_LEN];
        io::read_exact_at(&file, &mut bytes, 0)?;
        let header = Header::read(&bytes)?;

        let region = |offset: u64, len: u64, crc: u32, what: &str| -> Result<Vec<u8>> {
            if len > META_MAX.max(file_size)
                || offset.checked_add(len).is_none_or(|end| end > file_size)
            {
                return Err(Error::Corrupt(format!(
                    "{what} lies past the end of the file"
                )));
            }
            let mut out = vec![0; len as usize];
            io::read_exact_at(&file, &mut out, offset)?;
            if crc32c::crc32c(&out) != crc {
                return Err(Error::Corrupt(format!("{what} fails its CRC")));
            }
            Ok(out)
        };
        if header.meta_len as u64 > META_MAX {
            return Err(corrupt("metadata too large"));
        }
        let meta: Meta = from_cbor(&region(
            header.meta_offset,
            header.meta_len as u64,
            header.meta_crc,
            "metadata",
        )?)?;
        let index = region(
            header.index_offset,
            header.index_len,
            header.index_crc,
            "index",
        )?;
        if meta.media.size != header.logical_size {
            return Err(corrupt("metadata and header disagree on the disc's size"));
        }

        // Records, area by area, the areas tiling the disc
        let mut cursor = Cursor::new(&index);
        let mut areas = Vec::new();
        let mut stored_lens: Vec<u32> = Vec::new();
        let mut records_seen = 0u64;
        let mut at = 0u64;
        for area in &meta.media.areas {
            if area.start != at || area.len == 0 {
                return Err(corrupt("areas do not tile the disc"));
            }
            at = area
                .start
                .checked_add(area.len)
                .ok_or_else(|| corrupt("area too large"))?;
            let stored_len = match area.kind {
                AreaKind::Raw => area.len,
                AreaKind::WiiPartition => {
                    if !area.len.is_multiple_of(BLOCK_TOTAL_SIZE) {
                        return Err(corrupt("a partition of partial blocks"));
                    }
                    area.len / BLOCK_TOTAL_SIZE * BLOCK_DATA_SIZE
                }
            };
            let key = match (area.kind, &area.title_key) {
                (AreaKind::Raw, _) => None,
                (AreaKind::WiiPartition, Some(key)) => Some(
                    Key::try_from(key.as_slice())
                        .map_err(|_| corrupt("a title key not of 16 bytes"))?,
                ),
                (AreaKind::WiiPartition, None) => {
                    return Err(corrupt("a partition without its key"));
                }
            };
            let mut exceptions = HashMap::new();
            for exception in &area.exceptions {
                let hashes = HashBlock::try_from(exception.hashes.as_slice())
                    .map_err(|_| corrupt("a hash exception not of 1 KiB"))?;
                exceptions.insert(exception.block, hashes);
            }
            records_seen += area.records;
            if records_seen > header.chunk_count as u64 {
                return Err(corrupt("more records than the header counts"));
            }
            let mut starts = Vec::with_capacity(area.records as usize + 1);
            let mut records = Vec::with_capacity(area.records as usize);
            let mut position = 0u64;
            for _ in 0..area.records {
                let kind = cursor.u8()?;
                let len = cursor.uvarint()?;
                if len == 0 || len > RECORD_MAX {
                    return Err(corrupt("a record of a bad length"));
                }
                let record = match kind {
                    KIND_ZERO => Record::Zero,
                    KIND_GENERATED => {
                        if cursor.u8()? != GEN_GC_WII_FILLER {
                            return Err(Error::Unsupported("generator".into()));
                        }
                        Record::Padding(Box::new(cursor.take(SEED_BYTES)?.try_into().unwrap()))
                    }
                    KIND_SOLID => {
                        if len > GROUP_MAX {
                            return Err(corrupt("a chunk larger than a group"));
                        }
                        stored_lens.push(len as u32);
                        Record::Chunk(stored_lens.len() as u32 - 1)
                    }
                    KIND_DEDUP => {
                        let target = cursor.uvarint()?;
                        if target >= stored_lens.len() as u64
                            || stored_lens[target as usize] as u64 != len
                        {
                            return Err(corrupt("a reference to a chunk not stored before it"));
                        }
                        Record::Chunk(target as u32)
                    }
                    _ => return Err(Error::Unsupported(format!("record kind {kind}"))),
                };
                starts.push(position);
                records.push(record);
                position += len;
            }
            starts.push(position);
            if position != stored_len {
                return Err(corrupt("an area's records do not add up to it"));
            }
            areas.push(AreaIndex {
                start: area.start,
                len: area.len,
                kind: area.kind,
                key,
                exceptions,
                starts,
                records,
            });
        }
        if at != header.logical_size || records_seen != header.chunk_count as u64 {
            return Err(corrupt("the areas do not cover the disc"));
        }

        // Groups, then which chunk lies where
        let count = cursor.uvarint()?;
        let mut groups = Vec::new();
        let mut chunks = Vec::with_capacity(stored_lens.len());
        let mut position = HEADER_LEN as u64;
        let mut logical_offset = 0u64;
        let mut next = stored_lens.iter();
        for index in 0..count {
            let group = Group {
                codec: Codec::from_id(cursor.u8()?)?,
                chunks: cursor.uvarint()?,
                logical_len: cursor.uvarint()?,
                stored_len: cursor.uvarint()?,
            };
            if group.logical_len > GROUP_MAX
                || group.stored_len > 1 << 28
                || group.stored_len < group.logical_len / 1024
            {
                return Err(corrupt("a group of a bad size"));
            }
            let mut offset = 0u64;
            for _ in 0..group.chunks {
                let len = *next
                    .next()
                    .ok_or_else(|| corrupt("groups hold more chunks than stored"))?;
                chunks.push(ChunkAt {
                    group: index as u32,
                    offset: offset as u32,
                });
                offset += len as u64;
            }
            if offset != group.logical_len {
                return Err(corrupt("a group's chunks do not add up to it"));
            }
            let envelope_len = envelope_len(logical_offset, group.stored_len);
            groups.push(GroupAt {
                offset: position,
                envelope_len,
                group,
            });
            position += envelope_len + group.stored_len;
            logical_offset += group.logical_len;
        }
        if next.next().is_some() || !cursor.is_empty() {
            return Err(corrupt("the index holds more than it describes"));
        }
        if position > header.index_offset {
            return Err(corrupt("groups overlap the index"));
        }
        Ok(Self {
            file,
            header,
            meta,
            areas,
            chunks,
            groups,
            cache: HashMap::new(),
            wii: None,
        })
    }

    /// The size of the disc.
    pub fn size(&self) -> u64 {
        self.header.logical_size
    }

    pub fn profile(&self) -> Result<Profile> {
        Profile::from_id(self.header.profile)
    }

    pub fn codec(&self) -> Codec {
        self.meta.provenance.params.codec
    }

    /// The disc's hashes, as recorded when it was written: `blake3-256`,
    /// `sha-1`, `md5` and `crc-32`, each by its name. A claim, which
    /// [`decompress`] checks.
    pub fn identity(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.meta
            .identity
            .iter()
            .map(|identity| (identity.algo.as_str(), identity.value.as_slice()))
    }

    /// Read the disc at `offset`, failing past its end.
    pub fn read_at(&mut self, mut offset: u64, mut buf: &mut [u8]) -> Result<()> {
        if offset
            .checked_add(buf.len() as u64)
            .is_none_or(|end| end > self.size())
        {
            return Err(Error::Corrupt(format!(
                "read of {:#x} bytes at {offset:#x} past the end of the disc",
                buf.len()
            )));
        }
        while !buf.is_empty() {
            let index = self
                .areas
                .partition_point(|area| area.start + area.len <= offset);
            let area = &self.areas[index];
            let within = offset - area.start;
            let length = buf.len().min((area.len - within) as usize);
            match area.kind {
                AreaKind::Raw => self.read_stored(index, within, &mut buf[..length])?,
                AreaKind::WiiPartition => {
                    let group = within / GROUP_TOTAL_SIZE;
                    if !matches!(&self.wii, Some((a, g, _)) if *a == index && *g == group) {
                        let bytes = self.encrypt_group(index, group)?;
                        self.wii = Some((index, group, bytes));
                    }
                    let bytes = &self.wii.as_ref().unwrap().2;
                    let in_group = (within % GROUP_TOTAL_SIZE) as usize;
                    let length = length.min(bytes.len() - in_group);
                    buf[..length].copy_from_slice(&bytes[in_group..in_group + length]);
                    buf = &mut buf[length..];
                    offset += length as u64;
                    continue;
                }
            }
            buf = &mut buf[length..];
            offset += length as u64;
        }
        Ok(())
    }

    /// A Wii group as the disc holds it: its data hashed, the exceptions put
    /// back, and encrypted.
    fn encrypt_group(&mut self, area: usize, group: u64) -> Result<Vec<u8>> {
        let blocks = self.areas[area].len / BLOCK_TOTAL_SIZE;
        let count = (blocks - group * BLOCKS_PER_GROUP).min(BLOCKS_PER_GROUP) as usize;
        let mut data = vec![0; count * BLOCK_DATA_SIZE as usize];
        self.read_stored(area, group * GROUP_DATA_SIZE, &mut data)?;
        let mut blocks: Vec<BlockData> = data
            .chunks(BLOCK_DATA_SIZE as usize)
            .map(|block| block.try_into().unwrap())
            .collect();
        blocks.resize(BLOCKS_PER_GROUP as usize, [0; BLOCK_DATA_SIZE as usize]);
        let mut hashes = wii::hash_group(&blocks);
        let area = &self.areas[area];
        for (j, hashes) in hashes.iter_mut().enumerate().take(count) {
            if let Some(exception) = area.exceptions.get(&(group * BLOCKS_PER_GROUP + j as u64)) {
                *hashes = *exception;
            }
        }
        let mut out = vec![0; count * BLOCK_TOTAL_SIZE as usize];
        wii::encrypt_group(
            &area.key.expect("a partition has a key"),
            &hashes[..count],
            &blocks[..count],
            &mut out,
        );
        Ok(out)
    }

    /// Read an area's stored stream: a raw area's bytes, a partition's data.
    fn read_stored(&mut self, area: usize, mut offset: u64, mut buf: &mut [u8]) -> Result<()> {
        let mut record = self.areas[area]
            .starts
            .partition_point(|&start| start <= offset)
            - 1;
        while !buf.is_empty() {
            let (start, end) = {
                let starts = &self.areas[area].starts;
                (starts[record], starts[record + 1])
            };
            let within = offset - start;
            let length = buf.len().min((end - offset) as usize);
            let out = &mut buf[..length];
            match &self.areas[area].records[record] {
                Record::Zero => out.fill(0),
                Record::Padding(seed) => {
                    let mut lfg = Lfg::from_bytes(&seed[..]);
                    lfg.forward_bytes(((self.phase(area) + start) % BLOCK + within) as usize);
                    lfg.fill(out);
                }
                &Record::Chunk(id) => {
                    let chunk = self.chunks[id as usize];
                    let group = self.group(chunk.group)?;
                    let from = chunk.offset as usize + within as usize;
                    out.copy_from_slice(&group[from..from + length]);
                }
            }
            buf = &mut buf[length..];
            offset += length as u64;
            record += 1;
        }
        Ok(())
    }

    /// Where an area's generator counts from: the disc outside partitions,
    /// a partition's data within one.
    fn phase(&self, area: usize) -> u64 {
        let area = &self.areas[area];
        match area.kind {
            AreaKind::Raw => area.start,
            AreaKind::WiiPartition => 0,
        }
    }

    /// A group, decoded and checked against its envelope.
    fn group(&mut self, index: u32) -> Result<Arc<Vec<u8>>> {
        if let Some(group) = self.cache.get(&index) {
            return Ok(group.clone());
        }
        let at = &self.groups[index as usize];
        let mut bytes = vec![0; (at.envelope_len + at.group.stored_len) as usize];
        io::read_exact_at(&self.file, &mut bytes, at.offset)?;
        let (envelope, stored) = bytes.split_at(at.envelope_len as usize);
        let mut cursor = Cursor::new(envelope);
        if cursor.take(2)? != ENVELOPE_MAGIC
            || cursor.u8()? != KIND_SOLID
            || cursor.u8()? != at.group.codec.id() << 4
        {
            return Err(Error::Corrupt(format!("group {index}'s envelope")));
        }
        cursor.uvarint()?;
        cursor.uvarint()?;
        let crc = u32::from_be_bytes(cursor.take(4)?.try_into().unwrap());
        let data = codec::decompress(at.group.codec, stored, at.group.logical_len as usize)?;
        if crc32c::crc32c(&data) != crc {
            return Err(Error::Corrupt(format!("group {index} fails its CRC")));
        }
        // ponytail: forget every group past a few, an LRU if dedup-heavy
        // reads ever thrash
        if self.cache.len() >= 16 {
            self.cache.clear();
        }
        let data = Arc::new(data);
        self.cache.insert(index, data.clone());
        Ok(data)
    }
}

/// Write an ORE out as the disc it holds, checking it against the hash it
/// was written with.
///
/// `progress` is called with the input bytes consumed since the last call.
pub fn decompress(input: &Path, output: &Path, progress: &mut dyn FnMut(u64)) -> Result<Stats> {
    let input_size = std::fs::metadata(input)?.len();
    // A reader a core, each decoding slices of the disc, written in order
    let readers: Vec<Mutex<Reader>> = (0..rayon::current_num_threads())
        .map(|_| Reader::open(input).map(Mutex::new))
        .collect::<Result<_>>()?;
    let (size, logical_hash) = {
        let reader = readers[0].lock().unwrap();
        (reader.size(), reader.header.logical_hash)
    };
    let part = io::part(output);
    let result = (|| {
        let mut file = BufWriter::with_capacity(8 << 20, File::create(&part)?);
        let mut hasher = blake3::Hasher::new();
        let (mut offset, mut reported) = (0, 0);
        while offset < size {
            let slices: Vec<(u64, usize)> = (0..readers.len() as u64)
                .map(|i| offset + i * SLICE)
                .take_while(|&at| at < size)
                .map(|at| (at, SLICE.min(size - at) as usize))
                .collect();
            let decoded = slices
                .par_iter()
                .map(|&(at, length)| {
                    let worker = rayon::current_thread_index().unwrap_or(0) % readers.len();
                    let mut reader = readers[worker].lock().unwrap();
                    let mut buf = vec![0; length];
                    reader.read_at(at, &mut buf)?;
                    Ok(buf)
                })
                .collect::<Result<Vec<_>>>()?;
            for buf in decoded {
                hasher.update(&buf);
                file.write_all(&buf)?;
                offset += buf.len() as u64;
            }
            // Scale the progress through the disc to the file
            let consumed = (offset as u128 * input_size as u128 / size as u128) as u64;
            progress(consumed - reported);
            reported = consumed;
        }
        if *hasher.finalize().as_bytes() != logical_hash {
            return Err(corrupt(
                "the disc does not match the hash it was written with",
            ));
        }
        let file = file.into_inner().map_err(|e| e.into_error())?;
        file.sync_all()?;
        Ok(Stats {
            input_size,
            output_size: size,
        })
    })();
    io::finish(&part, output, result)
}

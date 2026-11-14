//! Reading an ORE as the disc it holds.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rayon::prelude::*;

use super::cd::sector::SECTOR;
use super::cdtrack::SectorMap;
use super::crypto::Key;
use super::error::{Error, Result};
use super::format::*;
use super::io;
use super::lfg::{Lfg, SEED_BYTES};
use super::pack::{Cache, read_group};
use super::segment::{BLOCK, RECORD_MAX, Run};
use super::store::{Index, NOWHERE, StoreReader, find_store, hex};
use super::wii::{
    self, BLOCK_DATA_SIZE, BLOCK_TOTAL_SIZE, BLOCKS_PER_GROUP, BlockData, GROUP_DATA_SIZE,
    GROUP_TOTAL_SIZE, HashBlock,
};
use super::writer::Stats;

/// What a core decodes at a time when writing a disc out: four Wii groups.
const SLICE: u64 = 4 * GROUP_TOTAL_SIZE;

#[derive(Clone)]
enum Record {
    Zero,
    Padding(Box<[u8; SEED_BYTES]>),
    Chunk(u32),
    /// A chunk of the store, by its id there.
    Store(u64),
}

/// What [`Reader::runs`] gives: an area starting, a CD data track's video
/// or XA plane starting, or a run.
pub(super) enum Event<'a> {
    /// Its kind, and the codec it takes when not the ORE's.
    Area(AreaKind, Option<Codec>),
    Video,
    Xa,
    Run(Run<'a>),
}

#[derive(Clone)]
struct AreaIndex {
    start: u64,
    len: u64,
    kind: AreaKind,
    /// A CD track's first sector, in frames from 00:00:00.
    frame: u32,
    /// A CD data track's sectors.
    map: Option<Arc<SectorMap>>,
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

#[derive(Clone)]
struct GroupAt {
    /// Where its envelope starts.
    offset: u64,
    envelope_len: u64,
    group: Group,
}

/// What a game weighs, in bytes. A standalone ORE's own, share and
/// standalone sizes are its file's; a thin one's count its chunks in the
/// store, each its part of its group's bytes, plus its file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sizes {
    /// The disc's.
    pub uncompressed: u64,
    /// What only this game uses: what deleting it frees, once the store
    /// is purged.
    pub own: u64,
    /// Every chunk it uses divided among the games using it: summed over a
    /// store's games, the store's size.
    pub share: u64,
    /// About what it would weigh detached.
    pub standalone: u64,
}

/// An ORE, read as the disc it holds.
pub struct Reader {
    file: File,
    file_size: u64,
    pub(super) header: Header,
    pub(super) meta: Meta,
    /// In the order their records come.
    areas: Vec<AreaIndex>,
    /// The areas' indexes, in disc order.
    by_start: Vec<usize>,
    chunks: Vec<ChunkAt>,
    groups: Vec<GroupAt>,
    cache: Cache,
    /// The store a thin ORE's chunks lie in, once found.
    store: Option<StoreReader>,
    /// The Wii group last encrypted: its area, its number, its bytes.
    wii: Option<(usize, u64, Vec<u8>)>,
}

fn corrupt(what: &str) -> Error {
    Error::Corrupt(what.into())
}

impl Reader {
    /// Open the ORE at `path`, and if it is thin, the store its chunks lie
    /// in: the nearest `.ore-store` up from its folder.
    pub fn open(path: &Path) -> Result<Self> {
        let mut reader = Self::inspect(path)?;
        if let Some(uuid) = reader.store_uuid() {
            let missing = |found: String| {
                Error::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("needs the ORE store {}, {found}", hex(&uuid)),
                ))
            };
            let dir = find_store(path).ok_or_else(|| missing("none found above it".into()))?;
            let index = Index::load(&dir)?;
            if index.uuid != uuid {
                return Err(missing(format!("not {}", dir.display())));
            }
            reader.attach_store(StoreReader::new(dir, Arc::new(index)))?;
        }
        Ok(reader)
    }

    /// Check a thin ORE's records against its store's index, and read
    /// through it.
    fn attach_store(&mut self, store: StoreReader) -> Result<()> {
        for area in &self.areas {
            for (record, window) in area.records.iter().zip(area.starts.windows(2)) {
                if let &Record::Store(id) = record {
                    let chunk = store.index.chunks.get(id as usize);
                    if chunk.is_none_or(|chunk| {
                        chunk.group == NOWHERE || chunk.len as u64 != window[1] - window[0]
                    }) {
                        return Err(Error::Corrupt(format!(
                            "chunk {id} is not in {}",
                            store.dir().display()
                        )));
                    }
                }
            }
        }
        self.store = Some(store);
        Ok(())
    }

    /// The ORE at `path`, without the store it may need: enough for its
    /// identity and the store it needs, not to read a thin one's disc.
    pub fn inspect(path: &Path) -> Result<Self> {
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
        let thin = header.flags_required & REQ_LIBRARY != 0;
        if thin != meta.store.is_some()
            || meta
                .store
                .as_ref()
                .is_some_and(|store| store.uuid.len() != 16)
        {
            return Err(corrupt("a thin ORE names no store"));
        }
        let mut last_id = 0u64;

        // Records, area by area, the areas, once sorted, tiling the disc
        let mut cursor = Cursor::new(&index);
        let mut areas = Vec::new();
        let mut stored_lens: Vec<u32> = Vec::new();
        let mut records_seen = 0u64;
        let mut by_start: Vec<usize> = (0..meta.media.areas.len()).collect();
        by_start.sort_by_key(|&i| meta.media.areas[i].start);
        let mut at = 0u64;
        for &i in &by_start {
            let area = &meta.media.areas[i];
            if area.start != at || area.len == 0 {
                return Err(corrupt("areas do not tile the disc"));
            }
            at = area
                .start
                .checked_add(area.len)
                .ok_or_else(|| corrupt("area too large"))?;
        }
        for area in &meta.media.areas {
            if matches!(area.kind, AreaKind::CdData | AreaKind::CdAudio)
                && (!area.len.is_multiple_of(SECTOR as u64)
                    || area.frame.is_none_or(|frame| frame > u32::MAX as u64 / 2))
            {
                return Err(corrupt("a CD track of partial sectors"));
            }
            let map = match (area.kind, &area.sectors) {
                (AreaKind::CdData, Some(sectors)) => Some(Arc::new(SectorMap::read(
                    sectors,
                    area.len / SECTOR as u64,
                )?)),
                (AreaKind::CdData, None) => return Err(corrupt("a CD data track without its map")),
                _ => None,
            };
            let stored_len = match area.kind {
                AreaKind::Raw | AreaKind::CdAudio => area.len,
                AreaKind::CdData => map.as_ref().unwrap().stored_len(),
                AreaKind::WiiPartition => {
                    if !area.len.is_multiple_of(BLOCK_TOTAL_SIZE) {
                        return Err(corrupt("a partition of partial blocks"));
                    }
                    area.len / BLOCK_TOTAL_SIZE * BLOCK_DATA_SIZE
                }
            };
            let key = match (area.kind, &area.title_key) {
                (AreaKind::Raw | AreaKind::CdData | AreaKind::CdAudio, _) => None,
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
                    KIND_STORE if thin => {
                        last_id = last_id
                            .checked_add_signed(unzigzag(cursor.uvarint()?))
                            .ok_or_else(|| corrupt("a chunk id out of range"))?;
                        Record::Store(last_id)
                    }
                    KIND_SOLID if !thin => {
                        if len > GROUP_MAX {
                            return Err(corrupt("a chunk larger than a group"));
                        }
                        stored_lens.push(len as u32);
                        Record::Chunk(stored_lens.len() as u32 - 1)
                    }
                    KIND_DEDUP if !thin => {
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
                frame: area.frame.unwrap_or(0) as u32,
                map,
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
            file_size,
            header,
            meta,
            areas,
            by_start,
            chunks,
            groups,
            cache: Cache::default(),
            store: None,
            wii: None,
        })
    }

    /// Another reader of the same ORE, sharing its store's index.
    pub fn try_clone(&self) -> Result<Self> {
        Ok(Self {
            file: self.file.try_clone()?,
            file_size: self.file_size,
            header: self.header.clone(),
            meta: self.meta.clone(),
            areas: self.areas.clone(),
            by_start: self.by_start.clone(),
            chunks: self.chunks.clone(),
            groups: self.groups.clone(),
            cache: Cache::default(),
            store: self
                .store
                .as_ref()
                .map(|store| StoreReader::new(store.dir().to_path_buf(), store.index.clone())),
            wii: None,
        })
    }

    /// The id of the store a thin ORE's chunks lie in.
    pub fn store_uuid(&self) -> Option<[u8; 16]> {
        let store = self.meta.store.as_ref()?;
        Some(store.uuid.as_slice().try_into().unwrap())
    }

    /// The chunks a thin ORE of the store `uuid` uses, each once.
    pub(super) fn store_ids(&self, uuid: &[u8; 16]) -> Option<Vec<u64>> {
        if self.store_uuid()? != *uuid {
            return None;
        }
        let mut ids: Vec<u64> = self
            .areas
            .iter()
            .flat_map(|area| &area.records)
            .filter_map(|record| match record {
                &Record::Store(id) => Some(id),
                _ => None,
            })
            .collect();
        ids.sort_unstable();
        ids.dedup();
        Some(ids)
    }

    /// What the game weighs.
    pub fn sizes(&self) -> Sizes {
        let file = self.file_size;
        let (mut own, mut share, mut standalone) = (file as f64, file as f64, file as f64);
        if let Some(store) = &self.store {
            let index = &store.index;
            for id in self.store_ids(&index.uuid).unwrap_or_default() {
                let chunk = &index.chunks[id as usize];
                let weight = index.weight(chunk);
                if chunk.refs <= 1 {
                    own += weight;
                }
                share += weight / chunk.refs.max(1) as f64;
                standalone += weight;
            }
        }
        Sizes {
            uncompressed: self.size(),
            own: own as u64,
            share: share as u64,
            standalone: standalone as u64,
        }
    }

    /// Every record as the run it was written from, in order, with how many
    /// bytes of disc are done once it is.
    pub(super) fn runs(&mut self, emit: &mut dyn FnMut(Event, u64) -> Result<()>) -> Result<()> {
        let mut start = 0;
        for area in 0..self.areas.len() {
            let (len, stored) = {
                let area = &self.areas[area];
                (area.len, *area.starts.last().unwrap())
            };
            let codec = self.meta.media.areas[area].codec;
            emit(Event::Area(self.areas[area].kind, codec), start)?;
            let planes = self.areas[area].map.as_ref().map(|map| map.plane_starts());
            for record in 0..self.areas[area].records.len() {
                let (from, to) = {
                    let starts = &self.areas[area].starts;
                    (starts[record], starts[record + 1])
                };
                if let Some(planes) = planes {
                    let at = start + (from as u128 * len as u128 / stored as u128) as u64;
                    if planes[1] == from {
                        emit(Event::Video, at)?;
                    }
                    if planes[2] == from {
                        emit(Event::Xa, at)?;
                    }
                }
                let length = to - from;
                let done = start + (to as u128 * len as u128 / stored as u128) as u64;
                match self.areas[area].records[record].clone() {
                    Record::Zero => emit(Event::Run(Run::Zero(length)), done)?,
                    Record::Padding(seed) => emit(Event::Run(Run::Padding(length, *seed)), done)?,
                    Record::Chunk(id) => {
                        let chunk = self.chunks[id as usize];
                        let group = self.group(chunk.group)?;
                        let at = chunk.offset as usize;
                        emit(
                            Event::Run(Run::Chunk(&group[at..at + length as usize])),
                            done,
                        )?;
                    }
                    Record::Store(id) => {
                        let store = self.store.as_mut().expect("a thin ORE has its store");
                        let (group, at) = store.chunk(id)?;
                        let bytes = &group[at..at + length as usize];
                        // What leaves the store is what went in
                        if blake3::hash(bytes).as_bytes()[..16]
                            != store.index.chunks[id as usize].digest
                        {
                            return Err(Error::Corrupt(format!(
                                "chunk {id} of the store does not match its hash"
                            )));
                        }
                        emit(Event::Run(Run::Chunk(bytes)), done)?;
                    }
                }
            }
            start += len;
        }
        Ok(())
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

    /// The disc's hashes, as recorded when it was written: what each
    /// covers, its algorithm and its value. A disc image's are `logical`,
    /// `blake3-256`, `sha-1`, `md5` and `crc-32`; a CD's are each file's,
    /// by its name, as a DAT lists them: its tracks, its cue sheet and its
    /// SBI, with the `logical` BLAKE3 of the tracks in order. A claim, which
    /// [`decompress`] checks.
    pub fn identity(&self) -> impl Iterator<Item = (&str, &str, &[u8])> {
        self.meta.identity.iter().map(|identity| {
            (
                identity.view.as_str(),
                identity.algo.as_str(),
                identity.value.as_slice(),
            )
        })
    }

    /// Whether it holds a CD, as a cue sheet and its tracks, rather than a
    /// disc image.
    pub fn is_cd(&self) -> bool {
        self.meta.media.cue.is_some()
    }

    /// The files [`decompress`] writes, given `output`: the disc image, or
    /// a CD's tracks beside its cue sheet, each with where it lies in the
    /// logical stream; and the files written whole, a CD's cue sheet and
    /// SBI.
    #[allow(clippy::type_complexity)]
    fn outputs(
        &self,
        output: &Path,
    ) -> Result<(Vec<(PathBuf, u64, u64)>, Vec<(PathBuf, Vec<u8>)>)> {
        let Some(cue) = &self.meta.media.cue else {
            return Ok((vec![(output.to_path_buf(), 0, self.size())], Vec::new()));
        };
        let dir = output.parent().unwrap_or(Path::new(""));
        let mut whole = vec![(output.to_path_buf(), cue.clone())];
        if let Some(sbi) = &self.meta.media.sbi {
            whole.push((output.with_extension("sbi"), sbi.clone()));
        }
        let mut tracks = Vec::new();
        for area in &self.meta.media.areas {
            let name = area.file.as_deref().unwrap_or_default();
            // A track's file lies beside the cue sheet, and nowhere else
            let plain = Path::new(name)
                .file_name()
                .is_some_and(|plain| plain == std::ffi::OsStr::new(name));
            let path = dir.join(name);
            if !plain
                || tracks.iter().any(|(other, _, _)| *other == path)
                || whole.iter().any(|(other, _)| *other == path)
            {
                return Err(Error::Corrupt(format!("a track's file name: {name:?}")));
            }
            tracks.push((path, area.start, area.len));
        }
        Ok((tracks, whole))
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
            let index = self.by_start[self
                .by_start
                .partition_point(|&i| self.areas[i].start + self.areas[i].len <= offset)];
            let area = &self.areas[index];
            let within = offset - area.start;
            let length = buf.len().min((area.len - within) as usize);
            match area.kind {
                AreaKind::Raw | AreaKind::CdAudio => {
                    self.read_stored(index, within, &mut buf[..length])?
                }
                AreaKind::CdData => self.read_sectors(index, within, &mut buf[..length])?,
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

    /// Read a CD data track at `within`: its sectors' data from the
    /// track's stream, the rest of them generated.
    fn read_sectors(&mut self, area: usize, within: u64, buf: &mut [u8]) -> Result<()> {
        let (map, frame) = {
            let area = &self.areas[area];
            (
                area.map.clone().expect("a CD data track has its map"),
                area.frame,
            )
        };
        let first = (within / SECTOR as u64) as usize;
        let end = (within + buf.len() as u64).div_ceil(SECTOR as u64) as usize;
        // The sectors' bytes in each plane, read in one go each
        let ranges = map.ranges(first..end);
        let mut planes = [Vec::new(), Vec::new(), Vec::new()];
        for (plane, range) in planes.iter_mut().zip(&ranges) {
            plane.resize((range.end - range.start) as usize, 0);
            self.read_stored(area, range.start, plane)?;
        }
        let mut sector = [0; SECTOR];
        let mut out = 0;
        for i in first..end {
            let (plane, at) = map.at(i);
            let from = ranges[plane].start;
            let data = &planes[plane][(at.start - from) as usize..(at.end - from) as usize];
            map.rebuild(i, frame + i as u32, data, &mut sector);
            // The part of the sector the read covers
            let start = (within + out as u64 - (i * SECTOR) as u64) as usize;
            let length = (SECTOR - start).min(buf.len() - out);
            buf[out..out + length].copy_from_slice(&sector[start..start + length]);
            out += length;
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
                &Record::Store(id) => {
                    let store = self.store.as_mut().ok_or_else(|| {
                        Error::Unsupported("a thin ORE read without its store".into())
                    })?;
                    let (group, at) = store.chunk(id)?;
                    let from = at + within as usize;
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
            AreaKind::WiiPartition | AreaKind::CdData | AreaKind::CdAudio => 0,
        }
    }

    /// A group, decoded and checked against its envelope.
    fn group(&mut self, index: u32) -> Result<Arc<Vec<u8>>> {
        let at = &self.groups[index as usize];
        let file = &self.file;
        self.cache.get(index, || {
            read_group(file, at.offset, at.envelope_len, &at.group)
        })
    }
}

/// Write an ORE out as the disc it holds, checking it against the hash it
/// was written with: a disc image to `output`; a CD to the cue sheet
/// `output`, its tracks beside it under the names the cue sheet gives
/// them, and its SBI, if it has one, of the cue sheet's name.
///
/// `progress` is called with the input bytes consumed since the last call.
pub fn decompress(input: &Path, output: &Path, progress: &mut dyn FnMut(u64)) -> Result<Stats> {
    let input_size = std::fs::metadata(input)?.len();
    // A reader a core, each decoding slices of the disc, written in order
    let first = Reader::open(input)?;
    let (files, whole) = first.outputs(output)?;
    let (size, logical_hash) = (first.size(), first.header.logical_hash);
    let mut readers = vec![];
    for _ in 1..rayon::current_num_threads() {
        readers.push(Mutex::new(first.try_clone()?));
    }
    readers.push(Mutex::new(first));
    let paths: Vec<&Path> = files
        .iter()
        .map(|(path, _, _)| path.as_path())
        .chain(whole.iter().map(|(path, _)| path.as_path()))
        .collect();
    let result = (|| {
        let mut hasher = blake3::Hasher::new();
        let (mut done, mut reported) = (0, 0);
        for (path, start, len) in &files {
            let mut file = BufWriter::with_capacity(8 << 20, File::create(io::part(path))?);
            let (mut offset, end) = (*start, start + len);
            while offset < end {
                let slices: Vec<(u64, usize)> = (0..readers.len() as u64)
                    .map(|i| offset + i * SLICE)
                    .take_while(|&at| at < end)
                    .map(|at| (at, SLICE.min(end - at) as usize))
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
                    done += buf.len() as u64;
                }
                // Scale the progress through the disc to the file
                let consumed = (done as u128 * input_size as u128 / size as u128) as u64;
                progress(consumed - reported);
                reported = consumed;
            }
            file.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        }
        if *hasher.finalize().as_bytes() != logical_hash {
            return Err(corrupt(
                "the disc does not match the hash it was written with",
            ));
        }
        let mut output_size = size;
        for (path, bytes) in &whole {
            let file = File::create(io::part(path))?;
            (&file).write_all(bytes)?;
            file.sync_all()?;
            output_size += bytes.len() as u64;
        }
        Ok(Stats {
            input_size,
            output_size,
        })
    })();
    // Every file into place, or none
    let result = result.and_then(|stats| {
        for path in &paths {
            std::fs::rename(io::part(path), path)?;
        }
        Ok(stats)
    });
    if result.is_err() {
        for path in &paths {
            let _ = std::fs::remove_file(io::part(path));
        }
    }
    result
}

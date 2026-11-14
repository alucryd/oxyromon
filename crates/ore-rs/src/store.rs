//! The store a library's games keep their chunks in: `.ore-store`, in the
//! directory of a system, holding `store.index`, its copy, and the segments
//! the groups lie in. A game stays a thin `.ore` of its own, naming its
//! chunks by their ids here.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::hash::{BuildHasher, Hasher};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rayon::prelude::*;

use super::error::{Error, Result};
use super::format::*;
use super::io;
use super::pack::{Cache, Packer, read_group};
use super::parity::{self, Layout};
use super::reader::Reader;
use super::writer::{Options, is_ore};

/// The name of a store's directory.
pub const STORE_DIR: &str = ".ore-store";
const INDEX: &str = "store.index";
const COPY: &str = "store.index.copy";
const LOCK: &str = "store.lock";

const INDEX_MAGIC: [u8; 4] = *b"ORSI";
const SEGMENT_MAGIC: [u8; 4] = *b"ORSS";
const VERSION: u16 = 1;
const INDEX_HEADER: usize = 56;
const SEGMENT_HEADER: u64 = 32;
const SEGMENT_RECORD: usize = 12;
const GROUP_RECORD: usize = 21;
const CHUNK_RECORD: usize = 32;
/// The group of a chunk no segment holds any more.
pub(super) const NOWHERE: u32 = u32::MAX;

/// How a store is made.
#[derive(Clone, Copy, Debug)]
pub struct StoreOptions {
    /// What a segment grows to before the next is started: under 4 GiB by
    /// default, as FAT32 takes no larger file. A segment holds one group at
    /// least, however small this is.
    pub segment_size: u64,
    /// The width of a chunk id, 32 or 64 bits: how many chunks the store
    /// may come to hold.
    pub id_bits: u8,
    /// Parity shards a segment, against its 239 data shards, from 0 to 16:
    /// how many of a row's blocks a check can rebuild. 16, the default,
    /// adds 6.7%; 8 adds 3.2%; 0, nothing, and rebuilds nothing.
    pub parity: u8,
}

impl Default for StoreOptions {
    fn default() -> Self {
        Self {
            segment_size: u32::MAX as u64,
            id_bits: 32,
            parity: 16,
        }
    }
}

pub(super) struct Segment {
    pub number: u32,
    /// What its parity covers: its header and groups.
    pub len: u64,
}

pub(super) struct StoreGroup {
    /// The number of the segment it lies in.
    pub segment: u32,
    /// Where its envelope starts there.
    pub offset: u64,
    pub group: Group,
}

impl StoreGroup {
    fn envelope_len(&self) -> u64 {
        envelope_len(0, self.group.stored_len)
    }
}

#[derive(Clone, Copy)]
pub(super) struct Chunk {
    /// The first half of its BLAKE3.
    pub digest: [u8; 16],
    pub group: u32,
    pub offset: u32,
    pub len: u32,
    /// How many games use it.
    pub refs: u32,
}

/// `store.index`: every chunk by its id, the groups they lie in, and the
/// segments those lie in.
pub(super) struct Index {
    pub uuid: [u8; 16],
    id_bits: u8,
    parity: u8,
    segment_size: u64,
    next_segment: u32,
    pub segments: Vec<Segment>,
    pub groups: Vec<StoreGroup>,
    pub chunks: Vec<Chunk>,
}

fn corrupt(what: &str) -> Error {
    Error::Corrupt(format!("store index: {what}"))
}

impl Index {
    fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            INDEX_HEADER
                + self.segments.len() * SEGMENT_RECORD
                + self.groups.len() * GROUP_RECORD
                + self.chunks.len() * CHUNK_RECORD
                + 4,
        );
        out.extend_from_slice(&INDEX_MAGIC);
        out.extend_from_slice(&VERSION.to_be_bytes());
        out.extend_from_slice(&[self.id_bits, self.parity]);
        out.extend_from_slice(&self.uuid);
        out.extend_from_slice(&self.segment_size.to_be_bytes());
        out.extend_from_slice(&self.next_segment.to_be_bytes());
        out.extend_from_slice(&(self.segments.len() as u32).to_be_bytes());
        out.extend_from_slice(&(self.groups.len() as u64).to_be_bytes());
        out.extend_from_slice(&(self.chunks.len() as u64).to_be_bytes());
        for segment in &self.segments {
            out.extend_from_slice(&segment.number.to_be_bytes());
            out.extend_from_slice(&segment.len.to_be_bytes());
        }
        for group in &self.groups {
            out.extend_from_slice(&group.segment.to_be_bytes());
            out.extend_from_slice(&group.offset.to_be_bytes());
            out.push(group.group.codec.id());
            out.extend_from_slice(&(group.group.logical_len as u32).to_be_bytes());
            out.extend_from_slice(&(group.group.stored_len as u32).to_be_bytes());
        }
        for chunk in &self.chunks {
            out.extend_from_slice(&chunk.digest);
            out.extend_from_slice(&chunk.group.to_be_bytes());
            out.extend_from_slice(&chunk.offset.to_be_bytes());
            out.extend_from_slice(&chunk.len.to_be_bytes());
            out.extend_from_slice(&chunk.refs.to_be_bytes());
        }
        let crc = crc32c::crc32c(&out);
        out.extend_from_slice(&crc.to_be_bytes());
        out
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < INDEX_HEADER + 4 {
            return Err(corrupt("cut short"));
        }
        if bytes[..4] != INDEX_MAGIC {
            return Err(Error::BadMagic {
                expected: "ORSI".into(),
                found: String::from_utf8_lossy(&bytes[..4]).into_owned(),
            });
        }
        let (body, crc) = bytes.split_at(bytes.len() - 4);
        if crc32c::crc32c(body).to_be_bytes() != crc {
            return Err(corrupt("fails its CRC"));
        }
        let mut cursor = Cursor::new(&body[4..]);
        let version = u16::from_be_bytes(cursor.be()?);
        if version != VERSION {
            return Err(Error::Unsupported(format!("store version {version}")));
        }
        let id_bits = cursor.u8()?;
        let parity = cursor.u8()?;
        if parity > 16 {
            return Err(corrupt("more than 16 parity shards"));
        }
        let uuid = cursor.be()?;
        let segment_size = u64::from_be_bytes(cursor.be()?);
        let next_segment = u32::from_be_bytes(cursor.be()?);
        let segment_count = u32::from_be_bytes(cursor.be()?) as u64;
        let group_count = u64::from_be_bytes(cursor.be()?);
        let chunk_count = u64::from_be_bytes(cursor.be()?);
        let expected = INDEX_HEADER as u128
            + segment_count as u128 * SEGMENT_RECORD as u128
            + group_count as u128 * GROUP_RECORD as u128
            + chunk_count as u128 * CHUNK_RECORD as u128;
        if body.len() as u128 != expected {
            return Err(corrupt("its tables do not add up to it"));
        }
        if !matches!(id_bits, 32 | 64)
            || chunk_count as u128 > 1 << id_bits
            || group_count >= NOWHERE as u64
        {
            return Err(corrupt("more than it may hold"));
        }
        let mut lens = HashMap::new();
        let mut segments = Vec::with_capacity(segment_count as usize);
        for _ in 0..segment_count {
            let segment = Segment {
                number: u32::from_be_bytes(cursor.be()?),
                len: u64::from_be_bytes(cursor.be()?),
            };
            if segment.number >= next_segment || lens.insert(segment.number, segment.len).is_some()
            {
                return Err(corrupt("a segment's number"));
            }
            segments.push(segment);
        }
        let mut groups = Vec::with_capacity(group_count as usize);
        for _ in 0..group_count {
            let group = StoreGroup {
                segment: u32::from_be_bytes(cursor.be()?),
                offset: u64::from_be_bytes(cursor.be()?),
                group: Group {
                    codec: Codec::from_id(cursor.u8()?)?,
                    chunks: 0,
                    logical_len: u32::from_be_bytes(cursor.be()?) as u64,
                    stored_len: u32::from_be_bytes(cursor.be()?) as u64,
                },
            };
            let end = group
                .offset
                .checked_add(group.envelope_len() + group.group.stored_len);
            if group.group.logical_len > GROUP_MAX
                || group.group.stored_len < group.group.logical_len / 1024
                || group.offset < SEGMENT_HEADER
                || lens
                    .get(&group.segment)
                    .is_none_or(|&len| end.is_none_or(|end| end > len))
            {
                return Err(corrupt("a group outside its segment"));
            }
            groups.push(group);
        }
        let mut chunks = Vec::with_capacity(chunk_count as usize);
        for _ in 0..chunk_count {
            let chunk = Chunk {
                digest: cursor.be()?,
                group: u32::from_be_bytes(cursor.be()?),
                offset: u32::from_be_bytes(cursor.be()?),
                len: u32::from_be_bytes(cursor.be()?),
                refs: u32::from_be_bytes(cursor.be()?),
            };
            if chunk.group != NOWHERE {
                let group = groups
                    .get_mut(chunk.group as usize)
                    .ok_or_else(|| corrupt("a chunk in no group"))?;
                if chunk.len == 0
                    || chunk.offset as u64 + chunk.len as u64 > group.group.logical_len
                {
                    return Err(corrupt("a chunk outside its group"));
                }
                group.group.chunks += 1;
            }
            chunks.push(chunk);
        }
        Ok(Self {
            uuid,
            id_bits,
            parity,
            segment_size,
            next_segment,
            segments,
            groups,
            chunks,
        })
    }

    /// Read a store's index, or its copy if it is missing or damaged.
    pub fn load(dir: &Path) -> Result<Self> {
        let read = |name| Self::from_bytes(&std::fs::read(dir.join(name))?);
        read(INDEX).or_else(|error| read(COPY).map_err(|_| error))
    }

    /// Write the index, then its copy, each replaced only by a whole one.
    fn save(&self, dir: &Path) -> Result<()> {
        let bytes = self.to_bytes();
        for name in [INDEX, COPY] {
            let path = dir.join(name);
            let part = io::part(&path);
            let mut file = File::create(&part)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            std::fs::rename(&part, &path)?;
        }
        Ok(())
    }

    /// What a chunk weighs: its part of its group's stored bytes, in
    /// proportion to its length, and of its segment's parity, in
    /// proportion to those.
    pub fn weight(&self, chunk: &Chunk) -> f64 {
        let at = &self.groups[chunk.group as usize];
        let group = &at.group;
        // Segments stay in the order of their numbers
        let sealed = self
            .segments
            .binary_search_by_key(&at.segment, |segment| segment.number)
            .map_or(1.0, |i| {
                let segment = &self.segments[i];
                self.file_len(segment) as f64 / segment.len as f64
            });
        group.stored_len as f64 * chunk.len as f64 / group.logical_len as f64 * sealed
    }

    /// The size of a segment's file, sealed.
    fn file_len(&self, segment: &Segment) -> u64 {
        segment.len + Layout::new(segment.len, self.parity).footer_len()
    }

    fn size(&self) -> u64 {
        self.segments
            .iter()
            .map(|segment| self.file_len(segment))
            .sum()
    }
}

fn segment_path(dir: &Path, number: u32) -> PathBuf {
    dir.join(format!("segment-{number:06}.ores"))
}

fn segment_header(uuid: &[u8; 16], number: u32) -> [u8; SEGMENT_HEADER as usize] {
    let mut out = [0; SEGMENT_HEADER as usize];
    out[0..4].copy_from_slice(&SEGMENT_MAGIC);
    out[4..6].copy_from_slice(&VERSION.to_be_bytes());
    out[8..12].copy_from_slice(&number.to_be_bytes());
    out[16..32].copy_from_slice(uuid);
    out
}

/// Open a segment to read, checking it is the store's.
fn open_segment(dir: &Path, uuid: &[u8; 16], number: u32) -> Result<File> {
    let file = File::open(segment_path(dir, number))?;
    let mut header = [0; SEGMENT_HEADER as usize];
    io::read_exact_at(&file, &mut header, 0)?;
    if header != segment_header(uuid, number) {
        return Err(Error::Corrupt(format!(
            "segment {number} is not this store's"
        )));
    }
    Ok(file)
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The store of the file at `path`: the nearest one up from its folder.
pub fn find_store(path: &Path) -> Option<PathBuf> {
    // Its folder is empty for a bare file name: the current directory
    std::path::absolute(path)
        .ok()?
        .ancestors()
        .skip(1)
        .map(|dir| dir.join(STORE_DIR))
        .find(|store| store.is_dir())
}

/// Make an empty store at `dir`, which is to be the `.ore-store` of a
/// system's directory.
pub fn create_store(dir: &Path, options: &StoreOptions) -> Result<()> {
    if !matches!(options.id_bits, 32 | 64) {
        return Err(Error::InvalidOption(
            "chunk ids are of 32 or 64 bits".into(),
        ));
    }
    if options.segment_size <= SEGMENT_HEADER {
        return Err(Error::InvalidOption("segments too small".into()));
    }
    if options.parity > 16 {
        return Err(Error::InvalidOption("parity of 16 shards at most".into()));
    }
    if dir.join(INDEX).exists() || dir.join(COPY).exists() {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("{} is a store already", dir.display()),
        )));
    }
    std::fs::create_dir_all(dir)?;
    // ponytail: an id from the process's hash seed, the time and the pid,
    // not a UUID by the book; a store only has to tell itself from another
    let mut hasher = blake3::Hasher::new();
    for _ in 0..2 {
        let random = std::collections::hash_map::RandomState::new();
        hasher.update(&random.build_hasher().finish().to_le_bytes());
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    hasher.update(&now.map_or(0, |now| now.as_nanos()).to_le_bytes());
    hasher.update(&std::process::id().to_le_bytes());
    Index {
        uuid: hasher.finalize().as_bytes()[..16].try_into().unwrap(),
        id_bits: options.id_bits,
        parity: options.parity,
        segment_size: options.segment_size,
        next_segment: 1,
        segments: Vec::new(),
        groups: Vec::new(),
        chunks: Vec::new(),
    }
    .save(dir)
}

/// A store being read: its index, shared by every reader of it, and the
/// groups last decoded.
// ponytail: readers take no lock, so a compaction or a write folding the
// last segment under one fails its reads (a segment gone) rather than wait
// for it; a shared lock, or the index loaded again, if that bites
pub(super) struct StoreReader {
    dir: PathBuf,
    pub index: Arc<Index>,
    files: HashMap<u32, File>,
    cache: Cache,
}

impl StoreReader {
    pub fn new(dir: PathBuf, index: Arc<Index>) -> Self {
        Self {
            dir,
            index,
            files: HashMap::new(),
            cache: Cache::default(),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The group a chunk lies in, decoded, and where it starts there.
    pub fn chunk(&mut self, id: u64) -> Result<(Arc<Vec<u8>>, usize)> {
        let chunk = self.index.chunks[id as usize];
        let at = &self.index.groups[chunk.group as usize];
        if !self.files.contains_key(&at.segment) {
            let file = open_segment(&self.dir, &self.index.uuid, at.segment)?;
            self.files.insert(at.segment, file);
        }
        let file = &self.files[&at.segment];
        let group = self.cache.get(chunk.group, || {
            read_group(file, at.offset, at.envelope_len(), &at.group)
        })?;
        Ok((group, chunk.offset as usize))
    }
}

/// Lock the store at `dir`, waiting for whoever holds it, until the file
/// returned is dropped.
fn lock(dir: &Path) -> Result<File> {
    let lock = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(dir.join(LOCK))?;
    lock.lock()?;
    Ok(lock)
}

/// A store being written to, which one writer does at a time: chunks added
/// to it are gathered into groups in segments of the writer's own, each
/// sealed as it is done, and its index saved once they all are written. A
/// sealed segment is never written to again, but to repair it.
pub(super) struct StoreWriter {
    dir: PathBuf,
    /// Held locked for as long as the writer lives.
    _lock: File,
    index: Index,
    // ponytail: every chunk's digest in memory, some 80 bytes a chunk with
    // the index, 5 GB for a library of a terabyte; a table sorted by digest
    // in the index, read in place, when a store grows that large
    ids: HashMap<[u8; 16], u64>,
    packer: Packer,
    /// The segment being written, the last the index lists.
    file: Option<File>,
    /// Bytes this writer added to the segments.
    added: u64,
    /// Whether its first segment starts with the store's last, if small.
    fold: bool,
    /// The segment folded into the one being written, gone on commit.
    folded: Option<u32>,
}

/// The largest last segment a write folds into its own, at most a sixteenth
/// of a segment: a write a small ROM would otherwise seal a segment and its
/// parity of its own, and folding more would copy too much each time. At 16
/// MiB, a Game Boy library written a ROM at a time wrote 14.3 GB for a 158
/// MB store; a compaction merges the segments of 4 MiB this leaves.
const FOLD_MAX: u64 = 4 << 20;

impl StoreWriter {
    /// Open the store at `dir`, waiting for any other writer to be done.
    /// New groups are compressed with `codec`, to `group_size` at most.
    pub fn open(dir: &Path, codec: Codec, group_size: usize) -> Result<Self> {
        let lock = lock(dir)?;
        let index = Index::load(dir)?;
        let ids = index
            .chunks
            .iter()
            .enumerate()
            .map(|(id, chunk)| (chunk.digest, id as u64))
            .collect();
        Ok(Self {
            dir: dir.to_path_buf(),
            _lock: lock,
            index,
            ids,
            packer: Packer::new(codec, group_size),
            file: None,
            added: 0,
            fold: true,
            folded: None,
        })
    }

    pub fn uuid(&self) -> [u8; 16] {
        self.index.uuid
    }

    /// What the chunks added next are compressed with.
    pub fn codec(&self) -> Codec {
        self.packer.codec
    }

    /// Compress the chunks added next with `codec`, the groups so far
    /// written first.
    pub fn set_codec(&mut self, codec: Codec) -> Result<()> {
        if self.packer.codec != codec {
            self.write(true)?;
            self.packer.set_codec(codec);
        }
        Ok(())
    }

    /// The id of the chunk `bytes`, whose BLAKE3 starts with `digest`,
    /// storing it if the store does not hold it.
    pub fn add(&mut self, digest: [u8; 16], bytes: &[u8]) -> u64 {
        let id = match self.ids.get(&digest) {
            Some(&id) if self.index.chunks[id as usize].group != NOWHERE => return id,
            // A chunk the store held once takes its id back
            Some(&id) => id,
            None => {
                let id = self.index.chunks.len() as u64;
                self.index.chunks.push(Chunk {
                    digest,
                    group: NOWHERE,
                    offset: 0,
                    len: 0,
                    refs: 0,
                });
                self.ids.insert(digest, id);
                id
            }
        };
        self.place(id, bytes);
        id
    }

    /// Count one more game as using a chunk.
    pub fn reference(&mut self, id: u64) {
        let refs = &mut self.index.chunks[id as usize].refs;
        *refs = refs.saturating_add(1);
    }

    /// Put a chunk's bytes in the group being filled.
    fn place(&mut self, id: u64, bytes: &[u8]) {
        let (pending, offset) = self.packer.push(bytes);
        let chunk = &mut self.index.chunks[id as usize];
        chunk.group = (self.index.groups.len() + pending) as u32;
        chunk.offset = offset;
        chunk.len = bytes.len() as u32;
    }

    /// Write the groups filled, or with `all` the last one too.
    pub fn write(&mut self, all: bool) -> Result<()> {
        for packed in self.packer.take(all)? {
            let group = packed.group;
            let envelope = envelope(group.codec, 0, group.stored_len, packed.crc);
            self.append(&envelope, &packed.stored, group)?;
        }
        Ok(())
    }

    /// Write a group at the end of the segment being written, or of a new
    /// one if that would take it, sealed, past a segment's size.
    fn append(&mut self, envelope: &[u8], stored: &[u8], group: Group) -> Result<()> {
        let need = (envelope.len() + stored.len()) as u64;
        let (parity, size) = (self.index.parity, self.index.segment_size);
        let full = |len: u64| len + need + Layout::new(len + need, parity).footer_len() > size;
        let len = self.index.segments.last().map_or(0, |segment| segment.len);
        if self.file.is_none() || (len > SEGMENT_HEADER && full(len)) {
            let fold = if self.file.is_none() && std::mem::take(&mut self.fold) {
                self.fold_candidate()?
            } else {
                None
            };
            self.new_segment()?;
            if let Some((number, groups)) = fold {
                self.fold_in(number, groups)?;
                // What was folded is sealed as it is if this would not fit
                if full(self.index.segments.last().unwrap().len) {
                    self.new_segment()?;
                }
            }
        }
        let segment = self.index.segments.last_mut().unwrap();
        let file = self.file.as_mut().unwrap();
        file.seek(SeekFrom::Start(segment.len))?;
        file.write_all(envelope)?;
        file.write_all(stored)?;
        self.index.groups.push(StoreGroup {
            segment: segment.number,
            offset: segment.len,
            group,
        });
        segment.len += need;
        self.added += need;
        Ok(())
    }

    /// The store's last segment, if small enough to fold, and its groups as
    /// they lie, each checked against its CRC: a segment that fails is left
    /// for its parity to repair, never copied where it has none.
    #[allow(clippy::type_complexity)]
    fn fold_candidate(&self) -> Result<Option<(u32, Vec<(usize, Vec<u8>)>)>> {
        let Some(last) = self.index.segments.last() else {
            return Ok(None);
        };
        if self.index.file_len(last) > FOLD_MAX.min(self.index.segment_size / 16) {
            return Ok(None);
        }
        let file = open_segment(&self.dir, &self.index.uuid, last.number)?;
        let mut groups: Vec<usize> = (0..self.index.groups.len())
            .filter(|&g| self.index.groups[g].segment == last.number)
            .collect();
        groups.sort_unstable_by_key(|&g| self.index.groups[g].offset);
        let mut raw = Vec::with_capacity(groups.len());
        for g in groups {
            let at = &self.index.groups[g];
            if read_group(&file, at.offset, at.envelope_len(), &at.group).is_err() {
                return Ok(None);
            }
            let mut bytes = vec![0; (at.envelope_len() + at.group.stored_len) as usize];
            io::read_exact_at(&file, &mut bytes, at.offset)?;
            raw.push((g, bytes));
        }
        Ok(Some((last.number, raw)))
    }

    /// Copy the groups of segment `number`, as they are, to the start of
    /// the segment just begun.
    fn fold_in(&mut self, number: u32, groups: Vec<(usize, Vec<u8>)>) -> Result<()> {
        let segment = self.index.segments.last_mut().unwrap();
        let file = self.file.as_mut().unwrap();
        for (g, bytes) in groups {
            file.seek(SeekFrom::Start(segment.len))?;
            file.write_all(&bytes)?;
            let group = &mut self.index.groups[g];
            group.segment = segment.number;
            group.offset = segment.len;
            segment.len += bytes.len() as u64;
        }
        self.folded = Some(number);
        Ok(())
    }

    /// Seal the segment being written, if any, and start the next.
    fn new_segment(&mut self) -> Result<()> {
        self.seal()?;
        let number = self.index.next_segment;
        self.index.next_segment = number
            .checked_add(1)
            .ok_or_else(|| Error::Unsupported("more than 2^32 segments".into()))?;
        // A segment a writer that failed left goes
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(segment_path(&self.dir, number))?;
        file.write_all(&segment_header(&self.index.uuid, number))?;
        self.index.segments.push(Segment {
            number,
            len: SEGMENT_HEADER,
        });
        self.file = Some(file);
        self.added += SEGMENT_HEADER;
        Ok(())
    }

    /// Give the segment being written its parity, and close it.
    fn seal(&mut self) -> Result<()> {
        if let Some(file) = self.file.take() {
            let segment = self.index.segments.last().unwrap();
            parity::seal(&file, segment.len, self.index.parity)?;
            file.sync_all()?;
            self.added += Layout::new(segment.len, self.index.parity).footer_len();
        }
        Ok(())
    }

    /// Write what is left to, then the index: only now does the store hold
    /// what was added. Returns the bytes added to its segments.
    pub fn commit(&mut self) -> Result<u64> {
        self.write(true)?;
        self.seal()?;
        if self.index.chunks.len() as u128 > 1 << self.index.id_bits
            || self.index.groups.len() >= NOWHERE as usize
        {
            return Err(Error::Unsupported(format!(
                "more chunks than a store of {}-bit ids holds",
                self.index.id_bits
            )));
        }
        let folded = self.folded.take();
        if let Some(number) = folded {
            self.index
                .segments
                .retain(|segment| segment.number != number);
        }
        self.index.save(&self.dir)?;
        // The index first, so it never lists a segment gone
        if let Some(number) = folded {
            match std::fs::remove_file(segment_path(&self.dir, number)) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
        Ok(self.added)
    }

    /// Count again how many games use each chunk, from the thin OREs
    /// beneath the store's parent directory, which is where a game finds
    /// its store from. Returns how many games there are.
    fn recount(&mut self) -> Result<u64> {
        let chunks = &mut self.index.chunks;
        for chunk in chunks.iter_mut() {
            chunk.refs = 0;
        }
        let mut games = 0;
        for_each_game(&self.dir, &self.index.uuid, &mut |path, ids| {
            for id in ids {
                match chunks.get_mut(id as usize) {
                    Some(chunk) if chunk.group != NOWHERE => {
                        chunk.refs = chunk.refs.saturating_add(1)
                    }
                    _ => {
                        return Err(Error::Corrupt(format!(
                            "{}: chunk {id} is not in its store",
                            path.display()
                        )));
                    }
                }
            }
            games += 1;
            Ok(())
        })?;
        Ok(games)
    }

    /// Copy what games still use of the segments at least `threshold` dead,
    /// by the bytes of their chunks, and of the small segments if there
    /// are several, into new segments, and drop them from the index. Their
    /// files go once the index is saved.
    fn compact_segments(&mut self, threshold: f64) -> Result<()> {
        // Dead and all bytes of chunks, by segment
        let mut bytes: HashMap<u32, (u64, u64)> = HashMap::new();
        for chunk in &self.index.chunks {
            if chunk.group != NOWHERE {
                let segment = self.index.groups[chunk.group as usize].segment;
                let (dead, all) = bytes.entry(segment).or_default();
                *all += chunk.len as u64;
                if chunk.refs == 0 {
                    *dead += chunk.len as u64;
                }
            }
        }
        // A segment a sixteenth full or less is small, as a commit that
        // adds little leaves one
        let small: Vec<u32> = self
            .index
            .segments
            .iter()
            .filter(|segment| self.index.file_len(segment) <= self.index.segment_size / 16)
            .map(|segment| segment.number)
            .collect();
        let mut dropped: HashSet<u32> = self
            .index
            .segments
            .iter()
            .filter(|segment| match bytes.get(&segment.number) {
                Some(&(dead, all)) => dead > 0 && dead as f64 >= threshold * all as f64,
                // No chunk left: nothing to keep it for
                None => true,
            })
            .map(|segment| segment.number)
            .collect();
        if small.len() > 1 {
            dropped.extend(small);
        }
        // A segment mostly of groups less than half full, several of them,
        // as games written a write each leave, which compress better packed
        // together
        let half = self.packer.group_size() as u64 / 2;
        let mut partial: HashMap<u32, (u64, u64, u64)> = HashMap::new();
        for group in &self.index.groups {
            let (count, bytes, all) = partial.entry(group.segment).or_default();
            *all += group.group.logical_len;
            if group.group.logical_len < half {
                *count += 1;
                *bytes += group.group.logical_len;
            }
        }
        dropped.extend(
            partial
                .iter()
                .filter(|&(_, &(count, bytes, all))| count > 1 && bytes * 2 >= all)
                .map(|(&segment, _)| segment),
        );
        if dropped.is_empty() {
            return Ok(());
        }

        // Their chunks, group by group, in the order they lie
        let mut moving: Vec<(u32, u32, u64)> = self
            .index
            .chunks
            .iter()
            .enumerate()
            .filter(|(_, chunk)| {
                chunk.group != NOWHERE
                    && dropped.contains(&self.index.groups[chunk.group as usize].segment)
            })
            .map(|(id, chunk)| (chunk.group, chunk.offset, id as u64))
            .collect();
        moving.sort_unstable();
        let mut files = HashMap::new();
        // Groups every chunk of which lives are copied as they are, once
        // the others' chunks are packed
        let mut whole = Vec::new();
        for run in moving.chunk_by(|a, b| a.0 == b.0) {
            let at = &self.index.groups[run[0].0 as usize];
            let (segment, offset, envelope_len, group) =
                (at.segment, at.offset, at.envelope_len(), at.group);
            let live: Vec<u64> = run
                .iter()
                .map(|&(_, _, id)| id)
                .filter(|&id| self.index.chunks[id as usize].refs > 0)
                .collect();
            if live.is_empty() {
                continue;
            }
            let file = match files.entry(segment) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    entry.insert(open_segment(&self.dir, &self.index.uuid, segment)?)
                }
            };
            // A group whole and half full or more is copied as it is
            if live.len() as u64 == group.chunks && group.logical_len >= half {
                whole.push((segment, offset, envelope_len, group, live));
                continue;
            }
            // Its chunks keep its codec: audio its coder, data what it was
            // written with
            let codec = group.codec;
            if self.packer.codec != codec {
                self.write(true)?;
                self.packer.set_codec(codec);
            }
            let data = read_group(file, offset, envelope_len, &group)?;
            for id in live {
                let chunk = self.index.chunks[id as usize];
                let (from, len) = (chunk.offset as usize, chunk.len as usize);
                self.place(id, &data[from..from + len]);
            }
            self.write(false)?;
        }
        self.write(true)?;
        for (segment, offset, envelope_len, group, live) in whole {
            let mut raw = vec![0; (envelope_len + group.stored_len) as usize];
            io::read_exact_at(&files[&segment], &mut raw, offset)?;
            let (envelope, stored) = raw.split_at(envelope_len as usize);
            self.append(envelope, stored, group)?;
            for id in live {
                self.index.chunks[id as usize].group = self.index.groups.len() as u32 - 1;
            }
        }

        // The groups left take new numbers; a chunk still in a group
        // dropped is nowhere
        let mut numbers = vec![NOWHERE; self.index.groups.len()];
        let mut kept = Vec::new();
        for (number, group) in std::mem::take(&mut self.index.groups)
            .into_iter()
            .enumerate()
        {
            if !dropped.contains(&group.segment) {
                numbers[number] = kept.len() as u32;
                kept.push(group);
            }
        }
        self.index.groups = kept;
        for chunk in &mut self.index.chunks {
            if chunk.group != NOWHERE {
                chunk.group = numbers[chunk.group as usize];
            }
        }
        for group in &mut self.index.groups {
            group.group.chunks = 0;
        }
        for chunk in &self.index.chunks {
            if chunk.group != NOWHERE {
                self.index.groups[chunk.group as usize].group.chunks += 1;
            }
        }
        self.index
            .segments
            .retain(|segment| !dropped.contains(&segment.number));
        Ok(())
    }

    /// Remove the segment files the index does not list.
    fn sweep(&self) -> Result<()> {
        let listed: HashSet<PathBuf> = self
            .index
            .segments
            .iter()
            .map(|segment| segment_path(&self.dir, segment.number))
            .collect();
        for entry in std::fs::read_dir(&self.dir)? {
            let path = entry?.path();
            let segment = path
                .extension()
                .is_some_and(|extension| extension == "ores");
            if segment && !listed.contains(&path) {
                std::fs::remove_file(&path)?;
            }
        }
        Ok(())
    }
}

/// What a compaction found and did.
#[derive(Debug, Clone, Copy)]
pub struct Compacted {
    /// The thin OREs that use the store.
    pub games: u64,
    /// The size of the store's segments, before and after.
    pub before: u64,
    pub after: u64,
}

/// Compact the store at `store`: count again which chunks the games beneath
/// its parent directory use, then compact the segments of which at least
/// `threshold`, from 0 to 1, is no game's any more, copying what is left of
/// them into a new one. Chunks that have to be compressed again are as
/// `options` say.
///
/// A game that names the store and cannot be read fails the compaction, and so
/// does one that names a chunk the store lacks.
pub fn compact(store: &Path, options: &Options, threshold: f64) -> Result<Compacted> {
    let mut writer = StoreWriter::open(store, options.codec, options.profile.group_size())?;
    // It copies the small segments itself
    writer.fold = false;
    let before = writer.index.size();
    let games = writer.recount()?;
    writer.compact_segments(threshold)?;
    let after = writer.index.size();
    // The index first, so it never lists a segment gone, then the
    // segments it no longer lists, the store still locked
    writer.commit()?;
    writer.sweep()?;
    Ok(Compacted {
        games,
        before,
        after,
    })
}

/// Call `f` with every thin ORE of the store `uuid` at `store` and the
/// chunks it uses, from beneath the store's parent directory, which is
/// where a game finds its store from. A game that cannot be read fails.
fn for_each_game(
    store: &Path,
    uuid: &[u8; 16],
    f: &mut dyn FnMut(&Path, Vec<u64>) -> Result<()>,
) -> Result<()> {
    fn beneath(
        dir: &Path,
        uuid: &[u8; 16],
        depth: u32,
        f: &mut dyn FnMut(&Path, Vec<u64>) -> Result<()>,
    ) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                // Links may loop
                if depth < 32 && path.file_name().is_some_and(|name| name != STORE_DIR) {
                    beneath(&path, uuid, depth + 1, f)?;
                }
                continue;
            }
            let ore = path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("ore"));
            if !ore || !is_ore(&path)? {
                continue;
            }
            let named = |error| Error::Corrupt(format!("{}: {error}", path.display()));
            let reader = Reader::inspect(&path).map_err(named)?;
            if let Some(ids) = reader.store_ids(uuid) {
                f(&path, ids)?;
            }
        }
        Ok(())
    }
    let store = std::path::absolute(store)?;
    beneath(store.parent().unwrap_or(&store), uuid, 0, f)
}

/// What checking a store found.
#[derive(Debug, Clone, Default)]
pub struct StoreChecked {
    pub segments: u64,
    /// Blocks of its segments that failed their CRC.
    pub damaged: u64,
    /// Of those, the blocks rebuilt from parity.
    pub repaired: u64,
    /// Groups damaged and not repaired: past what parity rebuilds, or
    /// without `repair`.
    pub lost_groups: u64,
    /// The games using a chunk of those groups.
    pub lost_games: Vec<PathBuf>,
}

/// Check the store at `store` whole: every segment against its CRCs, and
/// with `repair`, what fails them rebuilt from parity, in place; then
/// every group decoded, checked against its CRC, and every chunk against
/// its hash. Each chunk is read once, however many games share it.
pub fn check_store(store: &Path, repair: bool) -> Result<StoreChecked> {
    let _lock = lock(store)?;
    let index = Index::load(store)?;
    let mut checked = StoreChecked::default();
    let mut lost: HashSet<usize> = HashSet::new();
    let mut files = HashMap::new();
    for segment in &index.segments {
        checked.segments += 1;
        let groups = || {
            index
                .groups
                .iter()
                .enumerate()
                .filter(move |(_, group)| group.segment == segment.number)
        };
        let path = segment_path(store, segment.number);
        let Ok(file) = File::options().read(true).write(repair).open(&path) else {
            lost.extend(groups().map(|(number, _)| number));
            continue;
        };
        if index.parity > 0 {
            let found = parity::check(&file, &Layout::new(segment.len, index.parity), repair)?;
            checked.damaged += found.damaged;
            checked.repaired += found.repaired;
            for (number, group) in groups() {
                let end = group.offset + group.envelope_len() + group.group.stored_len;
                if found
                    .lost
                    .iter()
                    .any(|&(at, len)| at < end && group.offset < at + len)
                {
                    lost.insert(number);
                }
            }
        }
        let mut header = [0; SEGMENT_HEADER as usize];
        let read = io::read_exact_at(&file, &mut header, 0);
        if read.is_err() || header != segment_header(&index.uuid, segment.number) {
            lost.extend(groups().map(|(number, _)| number));
        }
        files.insert(segment.number, file);
    }

    let mut chunks: Vec<Vec<usize>> = vec![Vec::new(); index.groups.len()];
    for (id, chunk) in index.chunks.iter().enumerate() {
        if chunk.group != NOWHERE {
            chunks[chunk.group as usize].push(id);
        }
    }
    let bad: Vec<usize> = (0..index.groups.len())
        .into_par_iter()
        .filter(|number| !lost.contains(number))
        .filter(|&number| {
            let group = &index.groups[number];
            let Some(file) = files.get(&group.segment) else {
                return true;
            };
            match read_group(file, group.offset, group.envelope_len(), &group.group) {
                Err(_) => true,
                Ok(data) => chunks[number].iter().any(|&id| {
                    let chunk = &index.chunks[id];
                    let bytes = &data[chunk.offset as usize..][..chunk.len as usize];
                    blake3::hash(bytes).as_bytes()[..16] != chunk.digest
                }),
            }
        })
        .collect();
    lost.extend(bad);
    checked.lost_groups = lost.len() as u64;
    if !lost.is_empty() {
        let ids: HashSet<u64> = lost
            .iter()
            .flat_map(|&number| chunks[number].iter().map(|&id| id as u64))
            .collect();
        for_each_game(store, &index.uuid, &mut |path, used| {
            if used.iter().any(|id| ids.contains(id)) {
                checked.lost_games.push(path.to_path_buf());
            }
            Ok(())
        })?;
    }
    Ok(checked)
}

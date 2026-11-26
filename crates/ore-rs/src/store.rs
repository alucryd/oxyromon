// SPDX-License-Identifier: MIT OR Apache-2.0
//! The store a library's games keep their chunks in: `.ore-store`, in the
//! directory of a system, holding `store.index`, its copy, and the segments
//! the groups lie in. A game stays a thin `.ore` of its own, naming its
//! chunks by their ids here.

use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};

use rayon::prelude::*;

use super::error::{Error, Result};
use super::format::*;
use super::io;
use super::pack::{Cache, decode_group, read_group};
use super::parity::{self, Layout};
use super::precache::Warm;
use super::reader::Reader;
use super::reader::is_ore;

#[cfg(feature = "write")]
mod write;
#[cfg(feature = "write")]
pub(super) use write::StoreWriter;
#[cfg(feature = "write")]
pub use write::{Compacted, compact, create_store, recount, set_parity};

/// The name of a store's directory.
pub const STORE_DIR: &str = ".ore-store";
const INDEX: &str = "store.index";
const COPY: &str = "store.index.copy";
const LOCK: &str = "store.lock";

const INDEX_MAGIC: [u8; 4] = *b"ORSI";
const SEGMENT_MAGIC: [u8; 4] = *b"ORSS";
/// The index's version: 2 records each segment's parity, 1 one for all.
const VERSION: u16 = 2;
/// A segment's header's version, unchanged by the index's.
const SEGMENT_VERSION: u16 = 1;
const INDEX_HEADER: usize = 56;
const SEGMENT_HEADER: u64 = 32;
const SEGMENT_RECORD: usize = 13;
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
    /// Its parity shards, the store's when it was sealed.
    pub parity: u8,
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
/// segments those lie in. Opaque outside the crate: [`Index::load`] reads
/// one, and what it can answer is read through its methods, so a store's
/// index is loaded once and shared by everything that needs it.
pub struct Index {
    pub(crate) uuid: [u8; 16],
    id_bits: u8,
    /// Parity shards a segment sealed from now on is given.
    parity: u8,
    segment_size: u64,
    next_segment: u32,
    pub(crate) segments: Vec<Segment>,
    pub(crate) groups: Vec<StoreGroup>,
    pub(crate) chunks: Vec<Chunk>,
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
            out.push(segment.parity);
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
        if !matches!(version, 1 | VERSION) {
            return Err(Error::Unsupported(format!("store version {version}")));
        }
        // Version 1 gave every segment the store's parity
        let segment_record = if version == 1 { 12 } else { SEGMENT_RECORD };
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
            + segment_count as u128 * segment_record as u128
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
                parity: if version == 1 { parity } else { cursor.u8()? },
            };
            if segment.parity > 16 {
                return Err(corrupt("more than 16 parity shards"));
            }
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

    /// What a chunk weighs without the parity of its segment: its part of
    /// its group's stored bytes, in proportion to its length.
    pub(crate) fn data_weight(&self, chunk: &Chunk) -> f64 {
        let group = &self.groups[chunk.group as usize].group;
        group.stored_len as f64 * chunk.len as f64 / group.logical_len as f64
    }

    /// What a chunk weighs: its part of its group's stored bytes, in
    /// proportion to its length, and of its segment's parity, in
    /// proportion to those.
    pub(crate) fn weight(&self, chunk: &Chunk) -> f64 {
        let at = &self.groups[chunk.group as usize];
        // Segments stay in the order of their numbers
        let sealed = self
            .segments
            .binary_search_by_key(&at.segment, |segment| segment.number)
            .map_or(1.0, |i| {
                let segment = &self.segments[i];
                self.file_len(segment) as f64 / segment.len as f64
            });
        self.data_weight(chunk) * sealed
    }

    /// The size of a segment's file, sealed.
    fn file_len(&self, segment: &Segment) -> u64 {
        segment.len + Layout::new(segment.len, segment.parity).footer_len()
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
    out[4..6].copy_from_slice(&SEGMENT_VERSION.to_be_bytes());
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

/// A store being read: its index, shared by every reader of it, and the
/// groups last decoded.
// Readers take no lock: a segment a compaction or a fold removed under one
// is found again in the index, which the writer saves before removing it
pub(super) struct StoreReader {
    dir: PathBuf,
    pub index: Arc<Index>,
    files: HashMap<u32, File>,
    /// Shared with every clone of the reader this store belongs to.
    cache: Arc<Cache>,
    /// The compressed bytes a precache thread has brought in for this
    /// store, shared with every clone too; `None` when nothing was
    /// asked to precache.
    pub(super) warm: Option<Arc<Warm>>,
}

impl StoreReader {
    pub fn new(dir: PathBuf, index: Arc<Index>, cache_bytes: usize) -> Self {
        Self {
            dir,
            index,
            files: HashMap::new(),
            cache: Cache::shared(cache_bytes),
            warm: None,
        }
    }

    /// Another reader of the same store, sharing its index and its
    /// decoded groups; its segment files open as it needs them.
    pub fn shared(&self) -> Self {
        Self {
            dir: self.dir.clone(),
            index: self.index.clone(),
            files: HashMap::new(),
            cache: self.cache.clone(),
            warm: self.warm.clone(),
        }
    }

    /// The groups decoded from this store, counted once for all the
    /// readers sharing its cache.
    pub fn decodes(&self) -> u64 {
        self.cache.decodes()
    }

    /// Whether the cache holds this group: what a guesser asks before
    /// waking a prefetch for it.
    pub(super) fn holds(&self, group: u32) -> bool {
        self.cache.holds(group)
    }

    /// The groups of this store served from precached bytes rather than
    /// from a segment: a debug counter, summed by the reader's own.
    pub(super) fn warm_hits(&self) -> u64 {
        self.warm.as_ref().map_or(0, |w| w.hits())
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The group a chunk lies in, decoded, and where it starts there. A
    /// segment gone since the index was loaded, compacted or folded into
    /// another, sends it to the index again: a chunk keeps its id.
    pub fn chunk(&mut self, id: u64) -> Result<(Arc<Vec<u8>>, usize)> {
        match self.chunk_in_index(id) {
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                let index = Index::load(&self.dir)?;
                if index.uuid != self.index.uuid || id as usize >= index.chunks.len() {
                    return Err(Error::Io(e));
                }
                self.index = Arc::new(index);
                self.files.clear();
                self.cache.clear();
                if let Some(warm) = &self.warm {
                    // The groups moved; the positions they left hold
                    // other bytes now, or nothing.
                    warm.clear();
                }
                self.chunk_in_index(id)
            }
            result => result,
        }
    }

    fn chunk_in_index(&mut self, id: u64) -> Result<(Arc<Vec<u8>>, usize)> {
        let chunk = self.index.chunks[id as usize];
        let at = &self.index.groups[chunk.group as usize];
        let warm = self.warm.clone();
        if !self.files.contains_key(&at.segment) {
            let file = open_segment(&self.dir, &self.index.uuid, at.segment)?;
            self.files.insert(at.segment, file);
        }
        let file = &self.files[&at.segment];
        let group = self.cache.get(chunk.group, || {
            match warm.as_ref().and_then(|w| w.get((at.segment, at.offset))) {
                Some(bytes) => decode_group(&bytes, at.offset, at.envelope_len(), &at.group),
                None => read_group(file, at.offset, at.envelope_len(), &at.group),
            }
        })?;
        Ok((group, chunk.offset as usize))
    }

    /// The groups the chunks at `ids` lie in, each once, in the order a
    /// precache thread would cross them: segment first, position second.
    /// Ids the index no longer holds are skipped — a thread catches no
    /// ghosts, and a reader of such an id takes its chance at the file.
    pub(crate) fn precache_plan(&self, ids: &[u64]) -> Vec<(u32, u64, u64)> {
        let mut plan: Vec<(u32, u64, u64)> = ids
            .iter()
            .filter_map(|&id| {
                let chunk = self.index.chunks.get(id as usize)?;
                let at = self.index.groups.get(chunk.group as usize)?;
                Some((
                    at.segment,
                    at.offset,
                    at.envelope_len() + at.group.stored_len,
                ))
            })
            .collect();
        plan.sort_unstable();
        plan.dedup();
        plan
    }

    /// A handle on a segment's file for a precache thread to read
    /// through: the store opens it once, on the thread's own reader,
    /// and every call after hands out a fresh clone of it.
    pub(crate) fn file_for(&mut self, segment: u32) -> Result<File> {
        if !self.files.contains_key(&segment) {
            let file = open_segment(&self.dir, &self.index.uuid, segment)?;
            self.files.insert(segment, file);
        }
        Ok(self.files[&segment].try_clone()?)
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

/// Call `f` with every thin ORE of the store `uuid` at `store` and the
/// chunks it uses, from beneath the store's parent directory, which is
/// where a game finds its store from. A game that cannot be read fails,
/// unless `skip_unreadable`: a file whose ORE cannot be parsed then gives
/// no ids and no error, so it contributes nothing and the walk goes on.
fn for_each_game(
    store: &Path,
    uuid: &[u8; 16],
    skip_unreadable: bool,
    f: &mut dyn FnMut(&Path, Vec<u64>) -> Result<()>,
) -> Result<()> {
    fn beneath(
        dir: &Path,
        uuid: &[u8; 16],
        skip_unreadable: bool,
        depth: u32,
        f: &mut dyn FnMut(&Path, Vec<u64>) -> Result<()>,
    ) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                // Links may loop
                if depth < 32 && path.file_name().is_some_and(|name| name != STORE_DIR) {
                    beneath(&path, uuid, skip_unreadable, depth + 1, f)?;
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
            let reader = match Reader::inspect(&path) {
                Ok(reader) => reader,
                Err(_) if skip_unreadable => continue,
                Err(error) => return Err(named(error)),
            };
            if let Some(ids) = reader.store_ids(uuid) {
                f(&path, ids)?;
            }
        }
        Ok(())
    }
    let store = std::path::absolute(store)?;
    beneath(
        store.parent().unwrap_or(&store),
        uuid,
        skip_unreadable,
        0,
        f,
    )
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

/// Progress through `work` units of work, reported as `total` bytes: what
/// is reported adds up to `total` once finished, however far the work went.
pub(crate) struct Scaled<'a> {
    progress: &'a mut dyn FnMut(u64),
    total: u64,
    work: u64,
    done: u64,
    reported: u64,
}

impl<'a> Scaled<'a> {
    pub fn new(progress: &'a mut dyn FnMut(u64), total: u64, work: u64) -> Self {
        Self {
            progress,
            total,
            work: work.max(1),
            done: 0,
            reported: 0,
        }
    }

    pub fn advance(&mut self, n: u64) {
        self.done = (self.done + n).min(self.work);
        let consumed = (self.done as u128 * self.total as u128 / self.work as u128) as u64;
        (self.progress)(consumed - self.reported);
        self.reported = consumed;
    }

    pub fn finish(mut self) {
        self.advance(self.work);
    }
}

/// The size of the store at `store`: of the files in it, what
/// [`check_store`]'s progress adds up to.
pub fn store_size(store: &Path) -> Result<u64> {
    let mut size = 0;
    for entry in std::fs::read_dir(store)? {
        size += entry?.metadata()?.len();
    }
    Ok(size)
}

/// What a store weighs besides its data: what its chunks weigh, what
/// dedup keeps from being stored twice, and what parity adds, as
/// [`store_stats`] gives them, in bytes. Each follows the same ledger as
/// [`Sizes::share`]: bytes on disk, divided by the compression of the
/// group holding them and scaled up by the parity covering the segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StoreStats {
    /// The distinct chunks it holds.
    pub chunks: u64,
    /// What they weigh where they lie.
    pub stored: u64,
    /// What storing each chunk again for every game past the first would
    /// have added: the saving of dedup.
    pub dedup_saved: u64,
    /// What the footers of parity add to the segments.
    pub parity: u64,
}

/// What the store at `store` saves by dedup and what its parity costs:
/// its index read, its chunks and segments counted, no data.
pub fn store_stats(store: &Path) -> Result<StoreStats> {
    Ok(Index::load(store)?.stats())
}

impl Index {
    /// What the store this is the index of saves by dedup and what its
    /// parity costs, counted off the index alone: what [`store_stats`]
    /// answers, for a caller that already holds the index. Summed over the
    /// store's games, [`Index::savings`] adds up to these: the dedup of
    /// every game that names a chunk, and the parity paid by the games
    /// whose chunks lie in each segment.
    pub fn stats(&self) -> StoreStats {
        let mut stored = 0f64;
        let mut dedup_saved = 0f64;
        for chunk in &self.chunks {
            let weight = self.weight(chunk);
            stored += weight;
            if chunk.refs > 1 {
                // Every game past the first would have stored the chunk
                // again: what refs-1 copies of it weigh
                dedup_saved += weight * f64::from(chunk.refs - 1);
            }
        }
        let parity = self
            .segments
            .iter()
            .map(|segment| Layout::new(segment.len, segment.parity).footer_len())
            .sum();
        StoreStats {
            chunks: self.chunks.len() as u64,
            stored: stored.round() as u64,
            dedup_saved: dedup_saved.round() as u64,
            parity,
        }
    }
}

/// Check the store at `store` whole: every segment against its CRCs, and
/// with `repair`, what fails them rebuilt from parity, in place; then
/// every group decoded, checked against its CRC, and every chunk against
/// its hash. Each chunk is read once, however many games share it.
///
/// `progress` is called with the bytes checked since the last call, scaled
/// so the calls add up to its [`store_size`].
pub fn check_store(
    store: &Path,
    repair: bool,
    progress: &mut dyn FnMut(u64),
) -> Result<StoreChecked> {
    let _lock = lock(store)?;
    let total = store_size(store)?;
    let index = Index::load(store)?;
    // Segments with parity are read whole, then every group
    let work = index
        .segments
        .iter()
        .filter(|segment| segment.parity > 0)
        .map(|segment| Layout::new(segment.len, segment.parity).table_at())
        .chain(
            index
                .groups
                .iter()
                .map(|group| group.envelope_len() + group.group.stored_len),
        )
        .sum::<u64>();
    let mut scaled = Scaled::new(progress, total, work);
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
        if segment.parity > 0 {
            let layout = Layout::new(segment.len, segment.parity);
            let found = parity::check(&file, &layout, repair, &mut |n| scaled.advance(n))?;
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
    // Groups checked on every core, their progress reported from this thread
    let (sender, checked_len) = mpsc::channel();
    let (index, files, chunks, lost_ref) = (&index, &files, &chunks, &lost);
    let bad: Vec<usize> = std::thread::scope(|scope| {
        let checker = scope.spawn(move || {
            (0..index.groups.len())
                .into_par_iter()
                .filter(|number| !lost_ref.contains(number))
                .filter(|&number| {
                    let group = &index.groups[number];
                    let _ = sender.send(group.envelope_len() + group.group.stored_len);
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
                .collect()
        });
        for len in checked_len {
            scaled.advance(len);
        }
        checker.join().expect("the group checker panicked")
    });
    scaled.finish();
    lost.extend(bad);
    checked.lost_groups = lost.len() as u64;
    if !lost.is_empty() {
        let ids: HashSet<u64> = lost
            .iter()
            .flat_map(|&number| chunks[number].iter().map(|&id| id as u64))
            .collect();
        for_each_game(store, &index.uuid, false, &mut |path, used| {
            if used.iter().any(|id| ids.contains(id)) {
                checked.lost_games.push(path.to_path_buf());
            }
            Ok(())
        })?;
    }
    Ok(checked)
}

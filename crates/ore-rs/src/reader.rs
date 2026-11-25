// SPDX-License-Identifier: MIT OR Apache-2.0
//! Reading an ORE as the disc it holds.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, mpsc};

use rayon::prelude::*;

use super::cd::cue;
use super::cd::sector::SECTOR;
use super::cdtrack::SectorMap;
use super::crypto::Key;
use super::error::{Error, Result};
use super::format::*;
use super::format::{BLOCK, RECORD_MAX, Run};
use super::io;
use super::lfg::{Lfg, SEED_BYTES};
use super::pack::{Cache, decode_group, read_group};
use super::precache::{BATCH_BYTES, BATCH_PAUSE, Progress, Run as PrecacheRun, Warm};
use super::store::{Index, NOWHERE, StoreReader, find_store, hex};
use super::wii::{
    self, BLOCK_DATA_SIZE, BLOCK_TOTAL_SIZE, BLOCKS_PER_GROUP, BlockData, GROUP_DATA_SIZE,
    GROUP_TOTAL_SIZE, HashBlock,
};
use super::wiiu_crypt::Layers;

/// What a core decodes at a time when writing a disc out: four Wii groups.
const SLICE: u64 = 4 * GROUP_TOTAL_SIZE;

#[derive(Clone)]
enum Record {
    Zero,
    Padding(Box<[u8; SEED_BYTES]>),
    /// One byte repeated.
    Fill(u8),
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
    /// A Wii U region's encryption.
    wiiu: Option<Arc<Layers>>,
    /// A repeat's period.
    period: u64,
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

/// The read-ahead default: how many groups the worker keeps decoded ahead
/// of sequential reads. Two hides a slow Wi-Fi round trip at twice CD
/// speed; four is the same comfort with memory to spare.
pub const DEFAULT_READ_AHEAD: u32 = 4;

/// The read-ahead that keeps the ingot on one thread: reads plain and
/// wakes no worker. The answer for a frontend whose read callbacks cannot
/// answer a second thread, a VFS one day.
pub const READ_AHEAD_OFF: u32 = u32::MAX;

/// How far past the end of the last read a read may start and still be
/// sequential: a drive's reads overlap, and a track skips a few bytes.
const SEQUENTIAL_GAP: u64 = 64 << 10;

/// Sequential reads in a row before read-ahead engages: the third says
/// this is a playhead, not a seek passing through.
const SEQUENTIAL_HITS: u32 = 3;

/// The sectors of CD space a group of the data plane weighs: the window
/// a guess sweeps ahead. The video and XA planes weigh less per sector,
/// but the walk runs on until their groups fill, wherever they lie.
const SECTORS_PER_GROUP: usize = 512;

/// A group the read-ahead worker is told to have decoded: one of the
/// ORE's own, or a chunk whose group lies in the store. The store names
/// a chunk and not a group: fetching it through `StoreReader::chunk`
/// carries the reload a compaction demands with it.
#[derive(Clone, Copy)]
enum Fetch {
    Own(u32),
    Store(u64),
}

/// Read-ahead: the tracker deciding whether the reads are sequential,
/// and the worker that follows when they are. The worker works off a
/// queue of one guess, the newest read replacing the pending one and a
/// full queue dropping the send rather than waiting: it is the reader's
/// own read that computes the next guess, so a worker that falls behind
/// cannot fall behind forever. The sender dies with the reader: the
/// worker wakes to a dead channel, finishes the group in hand, and ends
/// without being joined, since `smelt_close` has no business waiting on
/// a group half-read over the network.
struct Ahead {
    /// The groups to keep ahead, 0 for never.
    window: usize,
    /// Where the last read ended, `None` until the first.
    last_end: Option<u64>,
    /// The reads since the last seek that started where the last ended.
    hits: u32,
    /// Whether the reads are sequential enough to be worth guessing at.
    on: bool,
    tx: Option<mpsc::SyncSender<Vec<Fetch>>>,
}

impl Ahead {
    fn off() -> Self {
        Self {
            window: 0,
            last_end: None,
            hits: 0,
            on: false,
            tx: None,
        }
    }

    /// Takes note of the read at `offset` for `len`: read-ahead should
    /// guess at what follows it now, if the answer is yes. A read that
    /// overlaps or trails the last by less than a track skip is
    /// sequential; anything else is a seek, and read-ahead starts
    /// counting again.
    fn watch(&mut self, offset: u64, len: u64) -> bool {
        if self.window == 0 || len == 0 {
            return false;
        }
        let end = offset + len;
        let sequential = self
            .last_end
            .is_some_and(|last| offset <= last + SEQUENTIAL_GAP && end >= last);
        self.hits = if sequential {
            (self.hits + 1).min(SEQUENTIAL_HITS)
        } else {
            0
        };
        self.on = self.hits >= SEQUENTIAL_HITS;
        // A seek cancels nothing: a batch already in the worker's hand
        // lands in the cache wherever it lies, where a stale one is
        // fenced and a live one is only early, and the next streak
        // guesses from the playhead's new home.
        self.last_end = Some(end);
        self.on
    }
}

/// What a game weighs, in bytes. A standalone ORE's own, share and
/// standalone sizes are its file's; a thin one's count its chunks in the
/// store, each its part of its group's bytes, plus its file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sizes {
    /// The disc's.
    pub uncompressed: u64,
    /// What only this game uses: what deleting it frees, once the store
    /// is compacted.
    pub own: u64,
    /// Every chunk it uses divided among the games using it: summed over a
    /// store's games, the store's size.
    pub share: u64,
    /// About what it would weigh detached.
    pub standalone: u64,
    /// The part of the parity footers of the segments its chunks lie in
    /// that it pays for: [`Sizes::share`] minus what its chunks would
    /// weigh with no footer. Summed over a store's games, the store's
    /// parity, give or take the envelope overhead and a last segment
    /// nothing has filled up.
    pub parity: u64,
}

/// What one game's chunks save and cost a store, in bytes, on the same
/// ledger as [`Sizes::share`]: [`Savings::dedup_saved`] is what dedup kept
/// from being written twice for this game, [`Savings::parity`] what its
/// share of the parity footers adds. Summed over every game of a store,
/// the two are what [`StoreStats`](crate::StoreStats) tells of the store.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Savings {
    /// [`Sizes::standalone`] minus [`Sizes::share`]: what this game saved
    /// by sharing its chunks with the games that name them too.
    pub dedup_saved: u64,
    /// Its [`Sizes::parity`].
    pub parity: u64,
}

/// The [`Sizes::share`] of each ORE at `paths` in the store at `store`, its
/// index loaded once for them all; `None` for one that can't be read. A
/// standalone ORE's, or one of another store's, is its file's size.
pub fn shares(store: &Path, paths: &[&Path]) -> Result<Vec<Option<u64>>> {
    Ok(Index::load(store)?.shares(paths))
}

/// What each ORE at `paths` gives its store and takes from it, as
/// [`Index::savings`] answers it: a standalone ORE's, or one of another
/// store's, saves nothing and pays nothing.
pub fn savings(store: &Path, paths: &[&Path]) -> Result<Vec<Option<Savings>>> {
    Ok(Index::load(store)?.savings(paths))
}

impl Index {
    /// The [`Sizes::share`] of each ORE at `paths` in the store this is
    /// the index of; `None` for one that can't be read: what [`shares`]
    /// answers, for a caller that already holds the index.
    pub fn shares(&self, paths: &[&Path]) -> Vec<Option<u64>> {
        paths
            .iter()
            .map(|path| {
                let reader = Reader::inspect(path).ok()?;
                Some(reader.sizes_in(Some(self))?.share)
            })
            .collect()
    }

    /// What each ORE at `paths` gives the store this is the index of and
    /// takes from it: what [`savings`] answers, for a caller that already
    /// holds the index. An ORE that cannot be read, or that names a chunk
    /// the store lacks, is `None`; a standalone one, or one of another
    /// store, saves nothing and pays nothing.
    pub fn savings(&self, paths: &[&Path]) -> Vec<Option<Savings>> {
        paths
            .iter()
            .map(|path| {
                let reader = Reader::inspect(path).ok()?;
                let sizes = reader.sizes_in(Some(self))?;
                Some(Savings {
                    dedup_saved: sizes.standalone - sizes.share,
                    parity: sizes.parity,
                })
            })
            .collect()
    }
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
    /// Shared with every clone of this reader, and with its read-ahead
    /// worker once it has one.
    cache: Arc<Cache>,
    /// The sequential-read tracker and the worker it may have started.
    ahead: Ahead,
    /// The compressed bytes of this ingot's own groups a precache
    /// thread has brought in; `None` when nothing was asked to precache.
    warm: Option<Arc<Warm>>,
    /// The precache run's counters, shared with every clone of the
    /// ingot opened with one: a frontend may poll the progress from
    /// the handle it reads on.
    precache: Option<Arc<PrecacheRun>>,
    /// Whether this reader is the one the run was started on: the only
    /// close that stops it, so a clone or a worker ending never stops
    /// a precache a brother still reads.
    precache_opener: bool,
    /// The store a thin ORE's chunks lie in, once found.
    store: Option<StoreReader>,
    /// The Wii group last encrypted: its area, its number, its bytes.
    wii: Option<(usize, u64, Vec<u8>)>,
    /// The Wii U span last encrypted: its area, where it starts, its bytes.
    wiiu: Option<(usize, u64, Vec<u8>)>,
}

impl Drop for Reader {
    fn drop(&mut self) {
        // A closed ingot should not leave a thread reading its whole
        // file over the network behind it: the precache notices the
        // stop between batches and gives up, unjoined, like its
        // read-ahead brother. Only the ingot the run was opened on
        // speaks; a clone closing says nothing.
        if let (true, Some(run)) = (self.precache_opener, &self.precache) {
            run.stop.store(true, Ordering::Relaxed);
        }
    }
}

fn corrupt(what: &str) -> Error {
    Error::Corrupt(what.into())
}

impl Reader {
    /// Open the ORE at `path`, and if it is thin, the store its chunks lie
    /// in: the nearest `.ore-store` up from its folder.
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with(path, 0)
    }

    /// [`Reader::open`] with a byte budget for the groups it keeps
    /// decoded; 0 asks for the default. An Android frontend over a slow
    /// share wants less than a desktop; the budget bounds the cache, not
    /// the reading. A thin ORE holds two caches, its groups' and its
    /// store's, each at this: the reader's total can reach twice it.
    /// Read-ahead runs at its default; [`Reader::open_ahead`] sets it.
    pub fn open_with(path: &Path, cache_bytes: usize) -> Result<Self> {
        Self::open_ahead(path, cache_bytes, 0)
    }

    /// [`Reader::open`] with a byte budget and a read-ahead window: the
    /// groups a background thread keeps decoded ahead of sequential
    /// reads, so the reads cross into bytes that already came back. 0
    /// asks for the default, [`DEFAULT_READ_AHEAD`]; [`READ_AHEAD_OFF`]
    /// reads with the caller's thread alone, which a frontend whose read
    /// callbacks cannot answer twice wants. The worker is internal to
    /// this reader: an ORE is still one caller's thread at a time.
    pub fn open_ahead(path: &Path, cache_bytes: usize, read_ahead: u32) -> Result<Self> {
        Self::open_opts(path, cache_bytes, read_ahead, false)
    }

    /// [`Reader::open`] with every knob: a byte budget, a read-ahead
    /// window, and the precache — a thread that reads the whole ingot's
    /// groups, compressed, into memory behind the playhead, so a game
    /// that seeks pays its round trips once rather than per group. The
    /// footprint is the file's, not the disc's, and decoding stays in
    /// the LRU. A thin ORE precaches only the store groups it uses. The
    /// thread is the ingot's: closing this reader stops it, and a
    /// thread that will not start leaves the ingot reading as if the
    /// precache had never been asked for.
    pub fn open_opts(
        path: &Path,
        cache_bytes: usize,
        read_ahead: u32,
        precache: bool,
    ) -> Result<Self> {
        let mut reader = Self::inspect_with(path, cache_bytes)?;
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
            reader.attach_store(StoreReader::new(dir, Arc::new(index), cache_bytes))?;
        }
        reader.ahead.window = match read_ahead {
            0 => DEFAULT_READ_AHEAD as usize,
            READ_AHEAD_OFF => 0,
            // The cap keeps a fanciful window out of the sector
            // arithmetic, and no playhead outruns sixteen groups.
            groups => groups.clamp(1, 16) as usize,
        };
        if precache {
            reader.start_precache();
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
        Self::inspect_with(path, 0)
    }

    /// [`Self::inspect`] with a byte budget for the groups it keeps
    /// decoded; 0 asks for the default.
    fn inspect_with(path: &Path, cache_bytes: usize) -> Result<Self> {
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
        if meta.media.class != header.media_class {
            return Err(corrupt("metadata and header disagree on the media class"));
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
            let wiiu = match (area.kind, &area.wiiu) {
                (AreaKind::Wiiu, Some(wiiu)) => Some(Arc::new(Layers::new(wiiu, area.len)?)),
                (AreaKind::Wiiu, None) => return Err(corrupt("a Wii U region without its layers")),
                _ => None,
            };
            // A repeat reads the disc a whole number of sectors before it,
            // never itself
            let period = match (area.kind, area.period) {
                (AreaKind::Repeat, Some(period))
                    if period > 0
                        && period.is_multiple_of(WIIU_SECTOR)
                        && period <= area.start
                        && area.records == 0 =>
                {
                    period
                }
                (AreaKind::Repeat, _) => return Err(corrupt("a repeat of a bad period")),
                _ => 0,
            };
            let stored_len = match area.kind {
                AreaKind::Raw | AreaKind::CdAudio | AreaKind::Wiiu => area.len,
                AreaKind::Repeat => 0,
                AreaKind::CdData => map.as_ref().unwrap().stored_len(),
                AreaKind::WiiPartition => {
                    if !area.len.is_multiple_of(BLOCK_TOTAL_SIZE) {
                        return Err(corrupt("a partition of partial blocks"));
                    }
                    area.len / BLOCK_TOTAL_SIZE * BLOCK_DATA_SIZE
                }
            };
            let key = match (area.kind, &area.title_key) {
                (
                    AreaKind::Raw
                    | AreaKind::CdData
                    | AreaKind::CdAudio
                    | AreaKind::Wiiu
                    | AreaKind::Repeat,
                    _,
                ) => None,
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
                    KIND_GENERATED => match cursor.u8()? {
                        GEN_GC_WII_FILLER => {
                            Record::Padding(Box::new(cursor.take(SEED_BYTES)?.try_into().unwrap()))
                        }
                        GEN_FILL => Record::Fill(cursor.u8()?),
                        _ => return Err(Error::Unsupported("generator".into())),
                    },
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
                wiiu,
                period,
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
        let mut position = u64::from(header.header_len);
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
            cache: Cache::shared(cache_bytes),
            ahead: Ahead::off(),
            warm: None,
            precache: None,
            precache_opener: false,
            store: None,
            wii: None,
            wiiu: None,
        })
    }

    /// Another reader of the same ORE, sharing its store's index and the
    /// groups both have decoded. The clone read-aheads on its own terms:
    /// the same window, detection from its own reads, and its own worker
    /// once those reads run sequential. It sees a precache run in flight
    /// (and never stops its precache): the run belongs to the ingot it
    /// was opened on.
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
            cache: self.cache.clone(),
            ahead: Ahead {
                window: self.ahead.window,
                ..Ahead::off()
            },
            warm: self.warm.clone(),
            precache: self.precache.clone(),
            precache_opener: false,
            store: self.store.as_ref().map(StoreReader::shared),
            wii: None,
            wiiu: None,
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
        self.sizes_in(self.store.as_ref().map(|store| &*store.index))
            .expect("attach_store checked every chunk is in the store")
    }

    /// What the game weighs, its chunks counted in `index`; `None` when one
    /// of them is not in it, as `attach_store` would refuse.
    fn sizes_in(&self, index: Option<&Index>) -> Option<Sizes> {
        let file = self.file_size;
        let (mut own, mut share, mut standalone, mut parity) =
            (file as f64, file as f64, file as f64, 0f64);
        if let Some(index) = index {
            for id in self.store_ids(&index.uuid).unwrap_or_default() {
                let chunk = index
                    .chunks
                    .get(id as usize)
                    .filter(|chunk| chunk.group != NOWHERE)?;
                let weight = index.weight(chunk);
                let refs = chunk.refs.max(1) as f64;
                if chunk.refs <= 1 {
                    own += weight;
                }
                share += weight / refs;
                standalone += weight;
                parity += (weight - index.data_weight(chunk)) / refs;
            }
        }
        Some(Sizes {
            uncompressed: self.size(),
            own: own as u64,
            share: share as u64,
            standalone: standalone as u64,
            parity: parity.round() as u64,
        })
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
                    Record::Fill(byte) => emit(Event::Run(Run::Fill(length, byte)), done)?,
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

    /// The platform it was written as: `gamecube`, `ps2` or `snes`, say,
    /// one of [`crate::CARTRIDGE_PLATFORMS`] for a cartridge's ROM.
    pub fn platform(&self) -> &str {
        &self.meta.media.platform
    }

    /// A cartridge ROM's file name, as it was written from.
    pub fn file_name(&self) -> Option<&str> {
        (self.header.media_class == MEDIA_ROM_CARTRIDGE)
            .then(|| self.meta.media.areas.first()?.file.as_deref())
            .flatten()
    }

    /// Whether it holds a CD, as a cue sheet and its tracks, rather than a
    /// disc image.
    pub fn is_cd(&self) -> bool {
        self.meta.media.cue.is_some()
    }

    /// Whether it holds a title's files, a Wii U eShop title's NUS folder,
    /// which [`decompress`] writes into its output, a folder.
    pub fn is_title(&self) -> bool {
        self.meta.media.class == MEDIA_CDN_TITLE
    }

    /// A title's files but its ticket, in the order the byte view holds
    /// them: each its name, and where it starts and how long it is there,
    /// so an emulator reads one as the folder's file; none for what is not
    /// a title.
    pub fn title_files(&self) -> Result<Vec<(String, u64, u64)>> {
        if !self.is_title() {
            return Ok(Vec::new());
        }
        let (files, _) = self.title_outputs(Path::new(""))?;
        Ok(files
            .into_iter()
            .map(|(path, start, len)| (path.to_string_lossy().into_owned(), start, len))
            .collect())
    }

    /// A title's ticket, its name and bytes, as its folder had it: none for
    /// a title written without one, or what is not a title.
    pub fn ticket(&self) -> Option<(&str, &[u8])> {
        let ticket = self
            .meta
            .media
            .ticket
            .as_ref()
            .filter(|_| self.is_title())?;
        Some((&ticket.name, &ticket.bytes))
    }

    /// A CD's tracks, as its cue sheet and its areas give them, in order;
    /// none for what is not a CD.
    pub fn tracks(&self) -> Result<Vec<Track>> {
        let Some(cue) = &self.meta.media.cue else {
            return Ok(Vec::new());
        };
        let sheet = cue::parse(&String::from_utf8_lossy(cue), Path::new(""))?;
        sheet
            .tracks
            .iter()
            .map(|track| {
                let name = track.file.file_name().and_then(|name| name.to_str());
                let area = self
                    .meta
                    .media
                    .areas
                    .iter()
                    .find(|area| area.file.as_deref() == name && area.frame.is_some())
                    .ok_or_else(|| corrupt("a cue sheet's file not in the ORE"))?;
                let index1 = track
                    .indexes
                    .iter()
                    .find(|&&(number, _)| number == 1)
                    .map_or(0, |&(_, frames)| u64::from(frames));
                Ok(Track {
                    number: track.number,
                    mode: match track.mode {
                        cue::Mode::Audio => TrackMode::Audio,
                        cue::Mode::Mode1 => TrackMode::Mode1,
                        cue::Mode::Mode2 => TrackMode::Mode2,
                    },
                    high_density: track.high_density,
                    flags: track.flags,
                    frame: area.frame.unwrap_or_default(),
                    index1,
                    sectors: area.len / SECTOR as u64,
                    offset: area.start,
                })
            })
            .collect()
    }

    /// A CD's SBI, the Q subchannel of the sectors where it differs from
    /// what its position generates, as libcrypt protection has it: each
    /// sector's frame from 00:00:00 and its 10 bytes of Q, without their
    /// CRC. Only full replacements, an SBI's type 1, which are what Redump
    /// writes; the partial ones, 2 and 3, are refused.
    pub fn subchannel_q(&self) -> Result<Vec<(u64, [u8; 10])>> {
        let Some(sbi) = &self.meta.media.sbi else {
            return Ok(Vec::new());
        };
        // An address's minutes, seconds and frames, each a BCD byte below
        // its bound
        let bcd = |byte: u8, below: u64| {
            let value = u64::from(byte >> 4) * 10 + u64::from(byte & 0xF);
            (byte >> 4 < 10 && byte & 0xF < 10 && value < below)
                .then_some(value)
                .ok_or_else(|| corrupt("an SBI's address"))
        };
        let mut entries = Vec::new();
        let mut rest = sbi
            .strip_prefix(b"SBI\0")
            .ok_or_else(|| corrupt("an SBI's magic"))?;
        while !rest.is_empty() {
            let (&[m, s, f, kind], tail) = rest
                .split_first_chunk::<4>()
                .ok_or_else(|| corrupt("an SBI cut short"))?;
            if kind != 1 {
                return Err(Error::Unsupported(format!("SBI entries of type {kind}")));
            }
            let (&q, tail) = tail
                .split_first_chunk::<10>()
                .ok_or_else(|| corrupt("an SBI cut short"))?;
            entries.push(((bcd(m, 100)? * 60 + bcd(s, 60)?) * 75 + bcd(f, 75)?, q));
            rest = tail;
        }
        Ok(entries)
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
        if self.is_title() {
            return self.title_outputs(output);
        }
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

    /// A title's files in the folder `output`, each from where its areas
    /// start to where they end, and its ticket, written whole.
    #[allow(clippy::type_complexity)]
    fn title_outputs(
        &self,
        output: &Path,
    ) -> Result<(Vec<(PathBuf, u64, u64)>, Vec<(PathBuf, Vec<u8>)>)> {
        let mut files: Vec<(PathBuf, u64, u64)> = Vec::new();
        let mut names = HashSet::new();
        // A file in the folder, and nowhere else, once
        let mut path = |name: &str| {
            let plain = Path::new(name)
                .file_name()
                .is_some_and(|plain| plain == std::ffi::OsStr::new(name));
            if !plain || !names.insert(name.to_owned()) {
                return Err(Error::Corrupt(format!("a title's file name: {name:?}")));
            }
            Ok(output.join(name))
        };
        let mut areas: Vec<&Area> = self.meta.media.areas.iter().collect();
        areas.sort_by_key(|area| area.start);
        for area in areas {
            let name = area.file.as_deref().unwrap_or_default();
            match files.last_mut() {
                Some((last, _, len)) if last.file_name() == Some(std::ffi::OsStr::new(name)) => {
                    *len += area.len
                }
                _ => files.push((path(name)?, area.start, area.len)),
            }
        }
        let mut whole = Vec::new();
        if let Some(ticket) = &self.meta.media.ticket {
            whole.push((path(&ticket.name)?, ticket.bytes.clone()));
        }
        Ok((files, whole))
    }

    /// Read the disc at `offset`, failing past its end. Reads that go on
    /// where the last left off switch on a read-ahead worker, decoding
    /// the groups after them into the shared cache before they are
    /// asked for: [`Reader::open_ahead`].
    pub fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        if offset
            .checked_add(buf.len() as u64)
            .is_none_or(|end| end > self.size())
        {
            return Err(Error::Corrupt(format!(
                "read of {:#x} bytes at {offset:#x} past the end of the disc",
                buf.len()
            )));
        }
        let ahead = self.ahead.watch(offset, buf.len() as u64);
        self.read_from(offset, buf)?;
        if ahead {
            self.prefetch(offset + buf.len() as u64);
        }
        Ok(())
    }

    /// The read itself: [`Self::read_at`] without the watcher, so a
    /// repeat's leap a period back is not mistaken for a seek.
    fn read_from(&mut self, mut offset: u64, mut buf: &mut [u8]) -> Result<()> {
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
                AreaKind::Wiiu => {
                    let to = within + length as u64;
                    if !matches!(&self.wiiu, Some((a, lo, bytes))
                        if *a == index && *lo <= within && lo + bytes.len() as u64 >= to)
                    {
                        let bytes = self.encrypt_span(index, within, to)?;
                        self.wiiu = Some((index, bytes.0, bytes.1));
                    }
                    let (_, lo, bytes) = self.wiiu.as_ref().unwrap();
                    let from = (within - lo) as usize;
                    buf[..length].copy_from_slice(&bytes[from..from + length]);
                }
                AreaKind::Repeat => {
                    // The disc a period before, which for a repeat longer
                    // than its period is the repeat itself: folded onto the
                    // period just before the area, so a repeat of any length
                    // reads in one step rather than one per period crossed,
                    // a recursion a disc's gigabytes of free space overflowed
                    // the stack with. `period <= start`, checked at open.
                    let period = area.period;
                    let phase = within % period;
                    let length = length.min((period - phase) as usize);
                    self.read_from(area.start - period + phase, &mut buf[..length])?;
                    buf = &mut buf[length..];
                    offset += length as u64;
                    continue;
                }
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

    /// A Wii U region's bytes from `from` to `to`, encrypted as the disc
    /// has them, and the outer layer's 64 KiB blocks around them: where
    /// they start, and their bytes.
    fn encrypt_span(&mut self, area: usize, from: u64, to: u64) -> Result<(u64, Vec<u8>)> {
        let layers = self.areas[area]
            .wiiu
            .clone()
            .expect("a Wii U region has its layers");
        let span = layers.span(self.areas[area].len, from, to);
        let mut bytes = vec![0; (span.hi - span.lo) as usize];
        self.read_stored(area, span.lo, &mut bytes)?;
        layers.encrypt(&span, &mut bytes);
        // What the inner blocks needed around it is the pieces' alone
        bytes.truncate((span.to - span.lo) as usize);
        bytes.drain(..(span.from - span.lo) as usize);
        Ok((span.from, bytes))
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
                &Record::Fill(byte) => out.fill(byte),
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
            AreaKind::WiiPartition
            | AreaKind::CdData
            | AreaKind::CdAudio
            | AreaKind::Wiiu
            | AreaKind::Repeat => 0,
        }
    }

    /// The groups decoded since this ORE opened: its own and its store's
    /// together, what a reader chasing a stutter counts against the
    /// sectors it has served.
    pub fn group_decodes(&self) -> u64 {
        self.cache.decodes() + self.store.as_ref().map_or(0, StoreReader::decodes)
    }

    /// What a precache in flight has done: `None` for an ingot opened
    /// without one, and for its clones alike only in that case — a clone
    /// of an ingot that was opened with one polls the same run. The run
    /// counts groups, the ingot's own and its store's together; a run
    /// that died is over and its `done` is all that ever came — a
    /// frontend waiting on completion can stop waiting when `running`
    /// says so.
    pub fn precache_progress(&self) -> Option<Progress> {
        let run = self.precache.as_ref()?;
        Some(Progress {
            done: run.done.load(Ordering::Relaxed),
            total: run.total,
            running: run.running.load(Ordering::Relaxed),
        })
    }

    /// Groups decoded from precached bytes rather than from their file,
    /// the ingot's own and its store's together: a debug counter, and
    /// the witness `tests/precache.rs` mutation-checks.
    pub fn precache_hits(&self) -> u64 {
        self.warm.as_ref().map_or(0, |w| w.hits())
            + self.store.as_ref().map_or(0, |s| s.warm_hits())
    }

    /// A group, decoded and checked against its envelope — from the
    /// precache's bytes if the decode needs bytes and the cache holds
    /// them, from the file otherwise. The lookup waits on the decode:
    /// a cached group touches neither.
    fn group(&mut self, index: u32) -> Result<Arc<Vec<u8>>> {
        let at = &self.groups[index as usize];
        let warm = self.warm.clone();
        let file = &self.file;
        self.cache.get(index, || {
            match warm.as_ref().and_then(|w| w.get((0, at.offset))) {
                Some(bytes) => decode_group(&bytes, at.offset, at.envelope_len, &at.group),
                None => read_group(file, at.offset, at.envelope_len, &at.group),
            }
        })
    }

    /// The clone the read-ahead worker reads through: its own file
    /// handle, the cache shared with its reader, and no read-ahead of
    /// its own.
    fn clone_for_worker(&self) -> Result<Self> {
        let mut worker = self.try_clone()?;
        worker.ahead = Ahead::off();
        // The run belongs to the ingot it was opened on: try_clone hands
        // a worker the counters but never the stop.
        Ok(worker)
    }

    /// Start the precache: the run and its byte store, then a thread
    /// reading through a worker clone — its own handle on the file, the
    /// store's files opened as its own. Nothing is attached before the
    /// thread is alive: a spawn that fails leaves no counters to chase.
    fn start_precache(&mut self) {
        let own: Vec<(u64, u64)> = self
            .groups
            .iter()
            .map(|at| (at.offset, at.envelope_len + at.group.stored_len))
            .collect();
        let plan = match &self.store {
            Some(store) => {
                let ids = self
                    .store_uuid()
                    .and_then(|uuid| self.store_ids(&uuid))
                    .unwrap_or_default();
                store.precache_plan(&ids)
            }
            None => Vec::new(),
        };
        let warm = Warm::new();
        let warm_store = if plan.is_empty() {
            None
        } else {
            Some(Warm::new())
        };
        let run = PrecacheRun::new(own.len() as u64 + plan.len() as u64);
        let Ok(worker) = self.clone_for_worker() else {
            return;
        };
        let started = std::thread::Builder::new()
            .name("ore-precache".into())
            .spawn({
                let (filled, filled_store, counted) =
                    (warm.clone(), warm_store.clone(), run.clone());
                move || {
                    Self::precache_loop(worker, own, plan, filled, filled_store, counted);
                }
            });
        if started.is_err() {
            // A thread that will not start leaves no counters to chase:
            // the ingot reads as if precache had never been asked for.
            return;
        }
        self.warm = Some(warm);
        if let Some(store) = &mut self.store {
            store.warm = warm_store;
        }
        self.precache = Some(run);
        self.precache_opener = true;
    }

    /// The thread's work: the ingot's own groups first, in file order,
    /// as many neighbours as a batch fits behind one read; then the
    /// store's, per segment the same way. Between batches it rests, so
    /// the playhead's reads get the share to themselves more often
    /// than not. The first error ends the run: bytes already in stay,
    /// and the ones that never came were never promised — a reader of
    /// those takes its chance at the file.
    fn precache_loop(
        worker: Reader,
        own: Vec<(u64, u64)>,
        plan: Vec<(u32, u64, u64)>,
        filled: Arc<Warm>,
        filled_store: Option<Arc<Warm>>,
        counted: Arc<PrecacheRun>,
    ) {
        let mut next = 0;
        while next < own.len() && !counted.stopped() {
            let start = own[next].0;
            let mut end = start + own[next].1;
            let mut upto = next + 1;
            while upto < own.len() && own[upto].0 + own[upto].1 - start <= BATCH_BYTES {
                end = own[upto].0 + own[upto].1;
                upto += 1;
            }
            let mut bytes = vec![0u8; (end - start) as usize];
            if io::read_exact_at(&worker.file, &mut bytes, start).is_err() {
                break;
            }
            for (offset, len) in &own[next..upto] {
                let from = (*offset - start) as usize;
                filled.insert((0, *offset), bytes[from..from + *len as usize].to_vec());
                counted.done.fetch_add(1, Ordering::Relaxed);
            }
            next = upto;
            std::thread::sleep(BATCH_PAUSE);
        }
        // The store phase runs only if the file itself answered: a disc
        // that will not read rarely has its store read better, and a
        // thread chasing one is what this isn't.
        let store_phase = filled_store.filter(|_| next == own.len());
        if let Some(filled_store) = store_phase {
            let mut store = worker;
            let mut next = 0;
            while next < plan.len() && !counted.stopped() {
                let segment = plan[next].0;
                let start = plan[next].1;
                let mut end = start + plan[next].2;
                let mut upto = next + 1;
                while upto < plan.len()
                    && plan[upto].0 == segment
                    && plan[upto].1 + plan[upto].2 - start <= BATCH_BYTES
                {
                    end = plan[upto].1 + plan[upto].2;
                    upto += 1;
                }
                // A store that vanished mid-run ends the run like a
                // failed read would: this thread has no watcher, so it
                // gets no panic path either.
                let Some(st) = store.store.as_mut() else {
                    break;
                };
                let Ok(file) = st.file_for(segment) else {
                    break;
                };
                let mut bytes = vec![0u8; (end - start) as usize];
                if io::read_exact_at(&file, &mut bytes, start).is_err() {
                    break;
                }
                for (segment, offset, len) in &plan[next..upto] {
                    let from = (*offset - start) as usize;
                    filled_store.insert(
                        (*segment, *offset),
                        bytes[from..from + *len as usize].to_vec(),
                    );
                    counted.done.fetch_add(1, Ordering::Relaxed);
                }
                next = upto;
                std::thread::sleep(BATCH_PAUSE);
            }
        }
        counted.finish();
    }

    /// Tells the worker to have decoded, as far as it gets, the groups
    /// the next read is likely to want. The worker starts with the first
    /// time read-ahead engages; a thread that will not start puts
    /// read-ahead to sleep rather than the read to trouble.
    fn prefetch(&mut self, end: u64) {
        let targets = self.guess(end);
        if targets.is_empty() {
            return;
        }
        if self.ahead.tx.is_none() {
            let (tx, rx) = mpsc::sync_channel(1);
            let Ok(worker) = self.clone_for_worker() else {
                self.ahead.window = 0;
                return;
            };
            if std::thread::Builder::new()
                .name("ore-read-ahead".into())
                .spawn(move || {
                    let mut worker = worker;
                    while let Ok(targets) = rx.recv() {
                        for fetch in targets {
                            // A failed guess says no more than that the
                            // guess was wrong: the reader who asked for
                            // the group reports its own failures.
                            match fetch {
                                Fetch::Own(group) => {
                                    let _ = worker.group(group);
                                }
                                Fetch::Store(chunk) => {
                                    if let Some(store) = &mut worker.store {
                                        let _ = store.chunk(chunk);
                                    }
                                }
                            }
                        }
                    }
                })
                .is_err()
            {
                self.ahead.window = 0;
                return;
            }
            self.ahead.tx = Some(tx);
        }
        // A full queue means the worker is still through its last batch:
        // the next read will guess again, and guess nearer the truth.
        let _ = self.ahead.tx.as_ref().unwrap().try_send(targets);
    }

    /// The groups the next read after `end` will want, near enough: the
    /// groups of the stored stream beyond it, up to the window, in
    /// logical space. A CD data track's next group in file order is
    /// often not its next group in use, so the guess starts from the
    /// sectors that follow and takes each plane of theirs, the records
    /// of their ranges, and the groups behind those. Groups already held
    /// still count toward the window but are not fetched again: a reader
    /// inside the worker's head start should guess nothing.
    fn guess(&self, end: u64) -> Vec<Fetch> {
        let mut out = Vec::new();
        let mut seen: Vec<(bool, u32)> = Vec::new();
        if end >= self.size() {
            return out;
        }
        let index = self.by_start[self
            .by_start
            .partition_point(|&i| self.areas[i].start + self.areas[i].len <= end)];
        let within = end - self.areas[index].start;
        match self.areas[index].kind {
            AreaKind::Raw | AreaKind::CdAudio | AreaKind::Wiiu => {
                self.guess_stored(index, within, &mut seen, &mut out)
            }
            AreaKind::CdData => {
                let map = self.areas[index]
                    .map
                    .as_ref()
                    .expect("a CD data track has its map");
                let from = (within / SECTOR as u64) as usize;
                if from < map.sectors() {
                    let to = (from + self.ahead.window * SECTORS_PER_GROUP).min(map.sectors());
                    // Each plane its window: the three advance as one
                    // sector stream, and the data plane's group ahead
                    // holds none of what the XA plane plays over it.
                    for range in map.ranges(from..to) {
                        self.guess_stored(index, range.start, &mut seen, &mut out);
                    }
                }
            }
            AreaKind::WiiPartition => {
                // The next whole group's data starts here in the stream;
                // the hashing and encryption after it are the reader's
                // to do, the stored groups behind are the worker's.
                let group = within / GROUP_TOTAL_SIZE;
                self.guess_stored(index, group * GROUP_DATA_SIZE, &mut seen, &mut out);
            }
            AreaKind::Repeat => {}
        }
        out
    }

    /// The groups of `area`'s stored stream from `from` onward, walking
    /// the records forward: the ORE's own chunks' groups, and store
    /// chunks' groups in the store, up to the group `from` lies in and
    /// the window past it. Every group walked counts toward the window,
    /// held or not, but only those no one holds yet, once each, are
    /// fetched.
    fn guess_stored(
        &self,
        area: usize,
        from: u64,
        seen: &mut Vec<(bool, u32)>,
        out: &mut Vec<Fetch>,
    ) {
        let area = &self.areas[area];
        let Some(&stream_end) = area.starts.last() else {
            return;
        };
        if from >= stream_end {
            return;
        }
        // The starts begin at zero: the record `from` lies in is the one
        // before the first start past it, whatever it holds.
        let mut record = area.starts.partition_point(|&start| start <= from) - 1;
        // The groups this walk has passed, held, seen or not: a plane's
        // window counts the groups another plane already gave.
        let mut walked: Vec<(bool, u32)> = Vec::new();
        while record < area.records.len() && walked.len() <= self.ahead.window {
            let (store_side, group, fetch) = match &area.records[record] {
                Record::Chunk(id) => {
                    let group = self.chunks[*id as usize].group;
                    (false, group, Fetch::Own(group))
                }
                Record::Store(id) => {
                    let id = *id;
                    let Some(chunk) = self
                        .store
                        .as_ref()
                        .and_then(|s| s.index.chunks.get(id as usize))
                    else {
                        break;
                    };
                    if chunk.group == NOWHERE {
                        record += 1;
                        continue;
                    }
                    (true, chunk.group, Fetch::Store(id))
                }
                _ => {
                    record += 1;
                    continue;
                }
            };
            record += 1;
            if walked.contains(&(store_side, group)) {
                continue;
            }
            walked.push((store_side, group));
            if seen.contains(&(store_side, group)) {
                continue;
            }
            seen.push((store_side, group));
            let held = if store_side {
                self.store.as_ref().is_some_and(|s| s.holds(group))
            } else {
                self.cache.holds(group)
            };
            if !held {
                out.push(fetch);
            }
        }
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
    // A reader a core, each decoding slices of the disc, written in order.
    // No read-ahead: a reader per core already covers the drive, and a
    // worker per one would only race them for the cache.
    let first = Reader::open_ahead(input, 0, READ_AHEAD_OFF)?;
    let (files, whole) = first.outputs(output)?;
    // A title goes in a folder, made for it and gone if it fails
    let made = first.is_title() && !output.exists();
    if made {
        std::fs::create_dir_all(output)?;
    }
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
            let slices = (offset..end)
                .step_by(SLICE as usize)
                .map(move |at| (at, SLICE.min(end - at) as usize));
            // Slices handed out in order, never more than `ahead` past the
            // one written, so the cores stay busy and the progress steady
            let ahead = 2 * readers.len();
            let (credit, credits) = mpsc::sync_channel(ahead);
            for _ in 0..ahead {
                let _ = credit.send(());
            }
            let (sender, decoded) = mpsc::channel();
            let readers = &readers;
            std::thread::scope(|scope| -> Result<()> {
                // Dropped on return, error or not, so the decoders stop
                let (credit, decoded) = (credit, decoded);
                scope.spawn(move || {
                    slices
                        .take_while(move |_| credits.recv().is_ok())
                        .par_bridge()
                        .for_each_with(sender, |sender, (at, length)| {
                            let worker = rayon::current_thread_index().unwrap_or(0) % readers.len();
                            let mut reader = readers[worker].lock().unwrap();
                            let mut buf = vec![0; length];
                            let _ = sender.send((at, reader.read_at(at, &mut buf).map(|()| buf)));
                        });
                });
                let mut pending = HashMap::new();
                while offset < end {
                    let (at, buf) = decoded.recv().expect("a decoder for every slice");
                    pending.insert(at, buf?);
                    while let Some(buf) = pending.remove(&offset) {
                        hasher.update(&buf);
                        file.write_all(&buf)?;
                        offset += buf.len() as u64;
                        done += buf.len() as u64;
                        let _ = credit.send(());
                        // Scale the progress through the disc to the file
                        let consumed = (done as u128 * input_size as u128 / size as u128) as u64;
                        progress(consumed - reported);
                        reported = consumed;
                    }
                }
                Ok(())
            })?;
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
        if made {
            let _ = std::fs::remove_dir(output);
        }
    }
    result
}

/// Sizes of the two files a run touched.
#[derive(Debug, Clone, Copy)]
pub struct Stats {
    pub input_size: u64,
    pub output_size: u64,
}

impl Stats {
    /// The output's size as a percentage of the input's.
    pub fn ratio_percent(&self) -> f64 {
        if self.input_size == 0 {
            0.0
        } else {
            self.output_size as f64 * 100.0 / self.input_size as f64
        }
    }
}

/// Whether `path` starts as an ORE does.
pub fn is_ore(path: &Path) -> Result<bool> {
    let file = File::open(path)?;
    let mut magic = [0; 4];
    Ok(io::read_exact_at(&file, &mut magic, 0).is_ok() && magic == MAGIC)
}

/// A CD track, as [`Reader::tracks`] gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub number: u8,
    pub mode: TrackMode,
    /// In a GD-ROM's high-density area.
    pub high_density: bool,
    /// Its cue sheet's `FLAGS`, as the Q subchannel's control bits: 4CH
    /// 0x8, DCP 0x2, PRE 0x1.
    pub flags: u8,
    /// Its file's first sector, in frames from 00:00:00: its LBA plus 150.
    pub frame: u64,
    /// Where its index 01 lies in its file, in sectors: the pregap the file
    /// holds before it.
    pub index1: u64,
    /// The sectors its file holds.
    pub sectors: u64,
    /// Where its file starts in what [`Reader::read_at`] reads, in bytes.
    pub offset: u64,
}

/// What a CD track's sectors hold, as its cue sheet says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackMode {
    Audio,
    Mode1,
    Mode2,
}

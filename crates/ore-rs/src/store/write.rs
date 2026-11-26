// SPDX-License-Identifier: GPL-3.0-or-later
//! Writing a store: making one, a writer adding groups to new segments and
//! sealing them, folding in the last small one, and compacting, all under
//! the store's lock. Built with the `write` feature.

use std::collections::hash_map::Entry;
use std::hash::{BuildHasher, Hasher};
use std::io::{Seek, SeekFrom};

use super::*;
use crate::pack::Packer;
use crate::writer::Options;

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

/// A store being written to, which one writer does at a time: chunks added
/// to it are gathered into groups in segments of the writer's own, each
/// sealed as it is done, and its index saved once they all are written. A
/// sealed segment is never written to again, but to repair it.
pub(crate) struct StoreWriter {
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

    /// Count one game fewer as using a chunk: the thin ORE that named it is
    /// gone, or is being written again over the one that did. An id the
    /// index does not hold and a chunk without references say nothing, as
    /// the count is already at the truth a recount would give.
    pub fn unreference(&mut self, id: u64) {
        if let Some(chunk) = self.index.chunks.get_mut(id as usize) {
            chunk.refs = chunk.refs.saturating_sub(1);
        }
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
        self.seal(&mut |_| {})?;
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
            parity: self.index.parity,
        });
        self.file = Some(file);
        self.added += SEGMENT_HEADER;
        Ok(())
    }

    /// Give the segment being written its parity, and close it.
    fn seal(&mut self, progress: &mut dyn FnMut(u64)) -> Result<()> {
        if let Some(file) = self.file.take() {
            let segment = self.index.segments.last().unwrap();
            parity::seal(&file, segment.len, segment.parity, progress)?;
            file.sync_all()?;
            self.added += Layout::new(segment.len, segment.parity).footer_len();
        }
        Ok(())
    }

    /// The bytes of the segment being written, if any.
    fn open_len(&self) -> u64 {
        match self.file {
            Some(_) => self.index.segments.last().map_or(0, |segment| segment.len),
            None => 0,
        }
    }

    /// What a commit writes and seals: the segment being written and what
    /// is still to be compressed, before it is.
    pub fn unsealed(&self) -> u64 {
        self.open_len() + self.packer.pending()
    }

    /// Write what is left to, then the index: only now does the store hold
    /// what was added. Returns the bytes added to its segments.
    ///
    /// `progress` is called with the bytes sealed since the last call,
    /// scaled so the calls add up to [`Self::unsealed`] before it.
    pub fn commit(&mut self, progress: &mut dyn FnMut(u64)) -> Result<u64> {
        let total = self.unsealed();
        self.write(true)?;
        let mut scaled = Scaled::new(progress, total, self.open_len());
        self.seal(&mut |n| scaled.advance(n))?;
        scaled.finish();
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
    /// its store from. Returns how many games there are. An ORE that
    /// cannot be read fails, unless `unreadable` is given, which counts it
    /// there and only the games it can read.
    fn recount(&mut self, unreadable: Option<&mut u64>) -> Result<u64> {
        let chunks = &mut self.index.chunks;
        for chunk in chunks.iter_mut() {
            chunk.refs = 0;
        }
        let mut games = 0;
        for_each_game(&self.dir, &self.index.uuid, unreadable, &mut |path, ids| {
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
    ///
    /// `progress` is called with the bytes of the groups read since the
    /// last call, scaled so the calls add up to the store's size.
    fn compact_segments(
        &mut self,
        threshold: f64,
        recompress: bool,
        progress: &mut dyn FnMut(u64),
    ) -> Result<()> {
        let total = store_size(&self.dir)?;
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
        if small.len() > 1 || recompress {
            dropped.extend(small);
        }
        if recompress {
            dropped.extend(self.index.segments.iter().map(|segment| segment.number));
        }
        // A segment sealed with other parity than the store's now
        dropped.extend(
            self.index
                .segments
                .iter()
                .filter(|segment| segment.parity != self.index.parity)
                .map(|segment| segment.number),
        );
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
            progress(total);
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
        let work = moving
            .chunk_by(|a, b| a.0 == b.0)
            .map(|run| {
                let at = &self.index.groups[run[0].0 as usize];
                at.envelope_len() + at.group.stored_len
            })
            .sum();
        let mut scaled = Scaled::new(progress, total, work);
        let mut files = HashMap::new();
        for run in moving.chunk_by(|a, b| a.0 == b.0) {
            let at = &self.index.groups[run[0].0 as usize];
            let (segment, offset, envelope_len, group) =
                (at.segment, at.offset, at.envelope_len(), at.group);
            scaled.advance(envelope_len + group.stored_len);
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
            if live.len() as u64 == group.chunks && group.logical_len >= half && !recompress {
                // Copied as it is where it lies, the chunks packed before it
                // written first, so the chunks keep their order for a later
                // recompression to pack as a batch would
                self.write(true)?;
                let mut raw = vec![0; (envelope_len + group.stored_len) as usize];
                io::read_exact_at(file, &mut raw, offset)?;
                let (envelope, stored) = raw.split_at(envelope_len as usize);
                self.append(envelope, stored, group)?;
                for id in live {
                    self.index.chunks[id as usize].group = self.index.groups.len() as u32 - 1;
                }
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
        scaled.finish();

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

/// What a recount found.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Recounted {
    /// The thin OREs that use the store.
    pub games: u64,
    /// The OREs beneath it whose header could not be read: whether they
    /// use the store is not known, so a store with none of `games` is
    /// only unused when it has none of these either.
    pub unreadable: u64,
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
/// them into a new one, and of segments mostly of groups less than half
/// full, packing those together again. With `recompress`, every segment is
/// written again and every group decoded and packed again, in the order
/// its chunks lie: the smallest the store gets, for a read and a write of
/// all of it. A chunk compressed again keeps its group's codec; `options`
/// give the groups' size.
///
/// A game that names the store and cannot be read fails the compaction, and so
/// does one that names a chunk the store lacks.
///
/// `progress` is called with the bytes of the groups copied since the last
/// call, scaled so the calls add up to its [`store_size`] before.
pub fn compact(
    store: &Path,
    options: &Options,
    threshold: f64,
    recompress: bool,
    progress: &mut dyn FnMut(u64),
) -> Result<Compacted> {
    let mut writer = StoreWriter::open(store, options.codec, options.profile.group_size())?;
    // It copies the small segments itself
    writer.fold = false;
    let before = writer.index.size();
    let games = writer.recount(None)?;
    writer.compact_segments(threshold, recompress, progress)?;
    let after = writer.index.size();
    // The index first, so it never lists a segment gone, then the
    // segments it no longer lists, the store still locked
    writer.commit(&mut |_| {})?;
    writer.sweep()?;
    Ok(Compacted {
        games,
        before,
        after,
    })
}

/// Count again how many games use each chunk of the store at `store`,
/// without touching its segments: what a store whose `refs` drifted from
/// its games needs, as a removal or a rewrite past the batch's own
/// bookkeeping leaves it, and a compaction does anyway. The index is
/// saved only when a count actually changed. Returns how many games the
/// store has, and how many OREs it could not tell about.
///
/// Unlike [`compact`], an ORE under the walk whose header cannot be read
/// is skipped, not fatal, counted in [`Recounted::unreadable`]: `check-roms`
/// moves a corrupt one to `Trash` beneath the walk root, and the recount
/// that follows must not sink every check, sync and purge after it. A
/// readable game that names a chunk the store lacks still fails it, as
/// in a compaction.
pub fn recount(store: &Path) -> Result<Recounted> {
    let mut writer = StoreWriter::open(store, Codec::Zstd, 1 << 20)?;
    // Nothing is written, so the last segment is not folded either
    writer.fold = false;
    let before: Vec<u32> = writer.index.chunks.iter().map(|chunk| chunk.refs).collect();
    let mut unreadable = 0;
    let games = writer.recount(Some(&mut unreadable))?;
    let changed = writer
        .index
        .chunks
        .iter()
        .zip(&before)
        .any(|(chunk, refs)| chunk.refs != *refs);
    if changed {
        writer.commit(&mut |_| {})?;
    }
    Ok(Recounted { games, unreadable })
}

/// Set the parity shards the store at `store` gives a segment, from 0 to
/// 16, as [`StoreOptions::parity`]: a segment sealed from now on takes it,
/// and [`compact`] writes those sealed with other parity again, copying
/// their groups as they are. Whether it changed.
pub fn set_parity(store: &Path, parity: u8) -> Result<bool> {
    if parity > 16 {
        return Err(Error::InvalidOption("parity of 16 shards at most".into()));
    }
    let mut writer = StoreWriter::open(store, Codec::Zstd, 1 << 20)?;
    if writer.index.parity == parity {
        return Ok(false);
    }
    writer.index.parity = parity;
    writer.commit(&mut |_| {})?;
    Ok(true)
}

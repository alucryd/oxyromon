// SPDX-License-Identifier: MIT OR Apache-2.0
//! Stored chunks gathered into groups, each compressed on its own, and
//! groups read back.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::sync::{Arc, Condvar, Mutex};

use rayon::prelude::*;

use super::codec;
use super::error::{Error, Result};
use super::format::*;
use super::io;

/// A group compressed, ready to be written behind its envelope.
pub struct Packed {
    pub group: Group,
    pub stored: Vec<u8>,
    /// The CRC-32C of what it decodes to.
    pub crc: u32,
}

/// Chunks gathered into groups, in the order they come, the groups
/// compressed several at once.
pub struct Packer {
    pub codec: Codec,
    group_size: usize,
    group: Vec<u8>,
    chunks: u64,
    /// Groups closed but not yet compressed.
    closed: Vec<(Vec<u8>, u64)>,
}

impl Packer {
    pub fn new(codec: Codec, group_size: usize) -> Self {
        Self {
            codec,
            group_size,
            group: Vec::with_capacity(group_size),
            chunks: 0,
            closed: Vec::new(),
        }
    }

    /// The bytes pushed and not yet taken.
    pub fn pending(&self) -> u64 {
        let closed: usize = self.closed.iter().map(|(data, _)| data.len()).sum();
        (self.group.len() + closed) as u64
    }

    /// The most bytes a group holds.
    pub fn group_size(&self) -> usize {
        self.group_size
    }

    /// Compress what comes next with `codec`, once every group so far is
    /// taken: a group is of one codec.
    pub fn set_codec(&mut self, codec: Codec) {
        debug_assert!(self.group.is_empty() && self.closed.is_empty());
        self.codec = codec;
    }

    /// Add a chunk: which group it lands in, counted from the first
    /// [`Self::take`] has yet to give, and where in it.
    pub fn push(&mut self, bytes: &[u8]) -> (usize, u32) {
        if self.group.len() + bytes.len() > self.group_size {
            self.close();
        }
        let offset = self.group.len() as u32;
        self.group.extend_from_slice(bytes);
        self.chunks += 1;
        (self.closed.len(), offset)
    }

    fn close(&mut self) {
        if self.chunks > 0 {
            let group = std::mem::replace(&mut self.group, Vec::with_capacity(self.group_size));
            self.closed.push((group, self.chunks));
            self.chunks = 0;
        }
    }

    /// The groups closed, compressed, in order: once there are enough to
    /// keep every core busy, or with `all`, every one, the last closed as
    /// it is.
    pub fn take(&mut self, all: bool) -> Result<Vec<Packed>> {
        if all {
            self.close();
        } else if self.closed.len() < rayon::current_num_threads() {
            return Ok(Vec::new());
        }
        let (codec, group_size) = (self.codec, self.group_size);
        std::mem::take(&mut self.closed)
            .into_par_iter()
            .map(|(data, chunks)| {
                let (codec, stored) = codec::compress_best(codec, &data, group_size)?;
                Ok(Packed {
                    group: Group {
                        codec,
                        chunks,
                        logical_len: data.len() as u64,
                        stored_len: stored.len() as u64,
                    },
                    stored,
                    crc: crc32c::crc32c(&data),
                })
            })
            .collect()
    }
}

/// Read the group whose envelope starts at `offset` of `file`, decoded and
/// checked against its envelope.
pub fn read_group(file: &File, offset: u64, envelope_len: u64, group: &Group) -> Result<Vec<u8>> {
    let mut bytes = vec![0; (envelope_len + group.stored_len) as usize];
    io::read_exact_at(file, &mut bytes, offset)?;
    let (envelope, stored) = bytes.split_at(envelope_len as usize);
    let mut cursor = Cursor::new(envelope);
    if cursor.take(2)? != ENVELOPE_MAGIC
        || cursor.u8()? != KIND_SOLID
        || cursor.u8()? != group.codec.id() << 4
    {
        return Err(Error::Corrupt(format!(
            "the envelope of the group at {offset:#x}"
        )));
    }
    cursor.uvarint()?;
    cursor.uvarint()?;
    let crc = u32::from_be_bytes(cursor.take(4)?.try_into().unwrap());
    let data = codec::decompress(group.codec, stored, group.logical_len as usize)?;
    if crc32c::crc32c(&data) != crc {
        return Err(Error::Corrupt(format!(
            "the group at {offset:#x} fails its CRC"
        )));
    }
    Ok(data)
}

/// What a cache holds when its owner asks for no budget: 32 MiB of
/// decoded groups, some 32 of the stream profile's 1 MiB ones.
pub const DEFAULT_CACHE_BYTES: usize = 32 << 20;

/// A decoded group, and the tick it was last read at: the LRU's order.
struct Entry {
    data: Arc<Vec<u8>>,
    used: u64,
}

struct Inner {
    /// The groups held, by id.
    groups: HashMap<u32, Entry>,
    /// What the groups weigh.
    bytes: usize,
    /// A tick, advanced on every read.
    clock: u64,
    /// The groups a thread is decoding right now.
    decoding: HashSet<u32>,
    /// The groups decoded since the cache was made, a debug counter: a
    /// reader that seeks well reads a group once, not once per sector.
    decodes: u64,
}

/// The groups last decoded, kept under a byte budget and evicted
/// least-recently-read first. An ORE and its store each hold one, and
/// clones of a reader share theirs, so a read-ahead worker can hold a
/// group in hand and the reader waiting on the same id takes it from the
/// cache instead of fetching and decoding it a second time.
pub struct Cache {
    budget: usize,
    inner: Mutex<Inner>,
    /// Woken when a group stops being decoded.
    ready: Condvar,
}

impl Default for Cache {
    fn default() -> Self {
        Self::new(DEFAULT_CACHE_BYTES)
    }
}

impl Cache {
    pub fn new(budget: usize) -> Self {
        Self {
            budget,
            inner: Mutex::new(Inner {
                groups: HashMap::new(),
                bytes: 0,
                clock: 0,
                decoding: HashSet::new(),
                decodes: 0,
            }),
            ready: Condvar::new(),
        }
    }

    /// A cache in the Arc an owner's clones, and its read-ahead worker
    /// later, share. A budget of 0 asks for the default.
    pub fn shared(bytes: usize) -> Arc<Self> {
        Arc::new(Self::new(if bytes == 0 {
            DEFAULT_CACHE_BYTES
        } else {
            bytes
        }))
    }

    /// The group this id, decoded: the one held if there is one, else
    /// what `decode` makes. A group another thread is decoding is
    /// waited for, not decoded a second time.
    pub fn get(
        &self,
        index: u32,
        decode: impl FnOnce() -> Result<Vec<u8>>,
    ) -> Result<Arc<Vec<u8>>> {
        let mut inner = self.inner.lock().unwrap();
        loop {
            inner.clock += 1;
            let tick = inner.clock;
            if let Some(entry) = inner.groups.get_mut(&index) {
                entry.used = tick;
                return Ok(entry.data.clone());
            }
            if inner.decoding.insert(index) {
                break;
            }
            inner = self.ready.wait(inner).unwrap();
        }
        // The claim stands, the lock does not: the guard would live to
        // the end of the function past the second lock below, and a
        // decode under it would hold every other reader hostage.
        drop(inner);
        let data = decode();
        let mut inner = self.inner.lock().unwrap();
        inner.decoding.remove(&index);
        self.ready.notify_all();
        let data = Arc::new(data?);
        inner.decodes += 1;
        inner.clock += 1;
        let weighed = data.capacity();
        // The least-recently-read groups leave until the new one fits.
        // One that out-weighs the budget alone takes the cache whole.
        while inner.bytes + weighed > self.budget {
            let Some((&oldest, _)) = inner.groups.iter().min_by_key(|(_, entry)| entry.used) else {
                break;
            };
            inner.bytes -= inner.groups.remove(&oldest).unwrap().data.capacity();
        }
        let entry = Entry {
            data: data.clone(),
            used: inner.clock,
        };
        if let Some(old) = inner.groups.insert(index, entry) {
            inner.bytes -= old.data.capacity();
        }
        inner.bytes += weighed;
        Ok(data)
    }

    /// Drops every group: a store whose index was reloaded holds groups
    /// that no longer mean what they did.
    pub fn clear(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.groups.clear();
        inner.bytes = 0;
    }

    /// The groups decoded since the cache was made.
    pub fn decodes(&self) -> u64 {
        self.inner.lock().unwrap().decodes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn the_least_recently_read_group_leaves_first() {
        let cache = Cache::new(300);
        let read = |cache: &Cache, id: u32| cache.get(id, || Ok(vec![id as u8; 100])).unwrap();
        read(&cache, 1);
        read(&cache, 2);
        read(&cache, 3);
        read(&cache, 1);
        read(&cache, 4);
        assert_eq!(cache.decodes(), 4);
        read(&cache, 1);
        read(&cache, 3);
        read(&cache, 4);
        assert_eq!(cache.decodes(), 4, "the three kept are the three read");
        read(&cache, 2);
        assert_eq!(cache.decodes(), 5, "the evicted one is decoded again");
    }

    #[test]
    fn a_group_over_the_budget_takes_the_cache_whole() {
        let cache = Cache::new(100);
        cache.get(1, || Ok(vec![1u8; 40])).unwrap();
        cache.get(2, || Ok(vec![2u8; 200])).unwrap();
        cache.get(2, || Ok(vec![2u8; 200])).unwrap();
        assert_eq!(cache.decodes(), 2, "the big group is kept, not refetched");
        cache.get(1, || Ok(vec![1u8; 40])).unwrap();
        assert_eq!(cache.decodes(), 3, "the small one went with its eviction");
    }

    #[test]
    fn a_group_being_decoded_is_decoded_once() {
        let cache = Arc::new(Cache::new(1 << 20));
        let decodes = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let (cache, decodes) = (Arc::clone(&cache), Arc::clone(&decodes));
                scope.spawn(move || {
                    cache
                        .get(7, || {
                            decodes.fetch_add(1, Ordering::SeqCst);
                            std::thread::sleep(std::time::Duration::from_millis(50));
                            Ok(vec![7u8; 4096])
                        })
                        .unwrap();
                });
            }
        });
        assert_eq!(decodes.load(Ordering::SeqCst), 1, "three waited, one read");
    }

    #[test]
    fn a_clear_forgets_groups_and_serves_them_again() {
        let cache = Cache::default();
        cache.get(1, || Ok(vec![1u8; 8])).unwrap();
        cache.clear();
        cache.get(1, || Ok(vec![2u8; 8])).unwrap();
        assert_eq!(cache.decodes(), 2);
    }
}

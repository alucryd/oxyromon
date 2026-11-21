// SPDX-License-Identifier: MIT OR Apache-2.0
//! Stored chunks gathered into groups, each compressed on its own, and
//! groups read back.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

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
    decode_group(&bytes, offset, envelope_len, group)
}

/// Decode a group from its envelope and stored bytes, checked against its
/// envelope. `offset` only names the group when it is broken: these bytes
/// came from the precache, whose holder has no file to consult.
pub fn decode_group(
    bytes: &[u8],
    offset: u64,
    envelope_len: u64,
    group: &Group,
) -> Result<Vec<u8>> {
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
    /// Bumped by every `clear()`: a decode that claimed before one may
    /// hold bytes a reloaded index no longer means.
    generation: u64,
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
                generation: 0,
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
    /// waited for, not decoded a second time. Bytes decoded across a
    /// `clear()` are given to their reader but not kept: a store that
    /// reloaded mid-decode may have moved the group under it.
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
        let mut claim = Claim {
            cache: self,
            index,
            generation: inner.generation,
            armed: true,
        };
        // The claim stands, the lock does not: the guard would live to
        // the end of the function past the second lock below, and a
        // decode under it would hold every other reader hostage.
        drop(inner);
        let data = decode();
        // The claim is released under the lock the handoff returns to,
        // so a waiter that wakes sees the group appear, never the gap
        // between released and inserted.
        let mut inner = self.inner.lock().unwrap();
        claim.disarm(&mut inner);
        let data = Arc::new(data?);
        inner.decodes += 1;
        if inner.generation != claim.generation {
            return Ok(data);
        }
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
        // Nothing waits behind the claim, and a clear empties rather
        // than replaces: the id is free here by construction.
        inner.groups.insert(index, entry);
        inner.bytes += weighed;
        Ok(data)
    }

    /// Whether the group this id is held, without claiming it or ticking
    /// its recency: a guesser checking whether a prefetch would be worth
    /// a wake-up at all.
    pub fn holds(&self, index: u32) -> bool {
        self.inner.lock().unwrap().groups.contains_key(&index)
    }

    /// Drops every group: a store whose index was reloaded holds groups
    /// that no longer mean what they did. Decodes claimed before this
    /// land nowhere but with their own reader.
    pub fn clear(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.groups.clear();
        inner.bytes = 0;
        inner.generation += 1;
    }

    /// The groups decoded since the cache was made.
    pub fn decodes(&self) -> u64 {
        self.inner.lock().unwrap().decodes
    }
}

/// A group claimed for decoding, armed until its reader is done with
/// it: a decode that leaves by panic drops the guard on the way out and
/// hands the id back, so the waiters retry instead of parking forever.
struct Claim<'a> {
    cache: &'a Cache,
    index: u32,
    /// The `clear()` count at claim time: another bump since means the
    /// bytes decoded are of an index that no longer exists.
    generation: u64,
    armed: bool,
}

impl Claim<'_> {
    /// Release the claim under a lock the return path holds anyway, so
    /// the wake-up and whatever the reader caches next are one hold: a
    /// waiter wakes on the group, not on the gap before it.
    fn disarm(&mut self, inner: &mut MutexGuard<'_, Inner>) {
        if self.armed {
            inner.decoding.remove(&self.index);
            self.cache.ready.notify_all();
            self.armed = false;
        }
    }
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        if self.armed {
            // Only a panic reaches here armed, and a lock poisoned by
            // that panic still holds the claim: taking the guard back
            // matters more than who poisoned it, for a Drop that panics
            // takes the whole process with it.
            let mut inner = self
                .cache
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            inner.decoding.remove(&self.index);
            self.cache.ready.notify_all();
        }
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

    #[test]
    fn a_decode_spanning_a_clear_is_returned_but_not_kept() {
        let cache = Cache::default();
        let stale = cache
            .get(1, || {
                // The lock is down while the group decodes: a store
                // can reload, and its clear, right through it.
                cache.clear();
                Ok(vec![9u8; 8])
            })
            .unwrap();
        assert_eq!(*stale, vec![9u8; 8], "the reader still gets its read");
        let fresh = cache.get(1, || Ok(vec![1u8; 8])).unwrap();
        assert_eq!(
            *fresh,
            vec![1u8; 8],
            "bytes of a dead index are not in the cache to be read"
        );
        assert_eq!(cache.decodes(), 2, "and so it is decoded again");
    }

    #[test]
    fn a_failed_decode_hands_the_id_to_the_waiter() {
        let cache = Arc::new(Cache::new(1 << 20));
        let (claimed, watching) = std::sync::mpsc::channel();
        let failing = {
            let (cache, claimed) = (Arc::clone(&cache), claimed);
            std::thread::spawn(move || {
                cache.get(7, || {
                    claimed.send(()).unwrap();
                    // Hold the claim until the waiter is parked on it,
                    // then fail: the release before the error returns
                    // is what the waiter lives on.
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    Err(Error::Corrupt("the group at 0 fails its CRC".into()))
                })
            })
        };
        watching.recv().unwrap();
        let waiting = {
            let cache = Arc::clone(&cache);
            std::thread::spawn(move || cache.get(7, || Ok(vec![7u8; 64])))
        };
        assert!(
            failing.join().unwrap().is_err(),
            "the failure comes back to its own reader alone"
        );
        let read = waiting
            .join()
            .unwrap()
            .expect("the waiter is not left hanging on it");
        assert_eq!(*read, vec![7u8; 64], "the waiter decodes for itself");
        assert_eq!(cache.decodes(), 1, "only the group that worked is kept");
    }

    #[test]
    fn a_panicking_decode_gives_the_group_back() {
        let cache = Arc::new(Cache::new(1 << 20));
        let reckless = {
            let cache = Arc::clone(&cache);
            std::thread::spawn(move || {
                std::panic::catch_unwind(|| cache.get(7, || panic!("the decoder left")))
            })
        };
        reckless.join().unwrap().unwrap_err();
        let read = cache.get(7, || Ok(vec![7u8; 64])).unwrap();
        assert_eq!(*read, vec![7u8; 64], "the id is claimable again");
    }
}

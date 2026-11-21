// SPDX-License-Identifier: MIT OR Apache-2.0
//! Precache: the ingot's groups, compressed, read into RAM behind the
//! playhead so that every later read decodes and nothing new seeks.
//!
//! Read-ahead follows a stream; a game that seeks — loading screens,
//! random file access — still pays a round trip per group on slow or
//! high-latency storage. A frontend whose CD access method is precache
//! wants the whole disc resident once, and a compressed PS1 or PC FX
//! ingot is a few hundred MiB: memory for the file, not for the disc.
//! So the bytes kept here are compressed; the decode cache stays the
//! LRU it is, and a group served from RAM is decoded exactly as one read
//! from disk would be — envelope, checksum and all.
//!
//! The thread is deliberately low priority: it reads in file order in
//! batches of several MiB, sleeps between them, claims nothing from any
//! cache, and stops at the first error rather than chasing one. What it
//! brings in lives under the position each group occupies — in the ORE
//! file, or in a store's segment — so a reader consults it by the same
//! address it would read by, and a position the thread never reached,
//! or that a compaction moved under it, is simply absent.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

/// How many bytes the thread asks for at a time. Group boundaries are
/// the ingot's business, not the share's: a span of eight MiB crosses
/// them without ceremony.
pub(crate) const BATCH_BYTES: u64 = 8 << 20;

/// How long the thread rests between batches: it serves no reader, and
/// the gap is where a read-ahead worker's round trip or a caller's miss
/// gets its answer first. Precaching a few hundred MiB pays a second
/// over the pure I/O; the playhead never waits for it.
pub(crate) const BATCH_PAUSE: std::time::Duration = std::time::Duration::from_millis(1);

/// A group's compressed bytes, by the position it occupies.
type Spans = Mutex<BTreeMap<(u32, u64), Arc<[u8]>>>;

/// The compressed bytes a precache thread has brought in, by the
/// position each group occupies: `(0, offset)` in an ORE file — the
/// ingot's own map takes no other key — and `(segment, offset)` in a
/// store, whose map takes no other. Two maps, never merged.
pub(crate) struct Warm {
    spans: Spans,
    /// Groups decoded from these bytes rather than the file: a debug
    /// counter, and the witness a test mutation-checks — break the
    /// lookup and it stops moving while the disc still reads.
    hits: AtomicU64,
}

impl Warm {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            spans: Mutex::new(BTreeMap::new()),
            hits: AtomicU64::new(0),
        })
    }

    /// One group's envelope and stored bytes, under its position.
    pub(crate) fn insert(&self, at: (u32, u64), bytes: Vec<u8>) {
        let mut spans = self.spans.lock().unwrap_or_else(PoisonError::into_inner);
        spans.insert(at, bytes.into());
    }

    /// The bytes a whole group spans, if the thread has reached them.
    pub(crate) fn get(&self, at: (u32, u64)) -> Option<Arc<[u8]>> {
        let found = self
            .spans
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&at)
            .cloned();
        if found.is_some() {
            self.hits.fetch_add(1, Ordering::Relaxed);
        }
        found
    }

    pub(crate) fn hits(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }

    /// Forget everything: a store whose index moved no longer keeps its
    /// groups where these positions say, and a stale byte is worse than
    /// a slow one.
    pub(crate) fn clear(&self) {
        self.spans
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }
}

/// What the thread has done and what it set out to do. The counts are
/// groups, the ingot's own and its store's alike; `done` only ever
/// reaches `total` if every batch was read.
pub(crate) struct Run {
    /// Groups brought in so far.
    pub(crate) done: AtomicU64,
    /// Groups the run set out to bring: counted at the open, when the
    /// index is fresh.
    pub(crate) total: u64,
    /// Whether the thread is still at it. A run that died on an error
    /// is over too, at whatever it managed — a frontend waiting on
    /// completion must be able to stop waiting.
    pub(crate) running: AtomicBool,
    /// Set when the reader closes: the thread checks it between batches
    /// and gives up, so a closed ingot leaves no thread reading a
    /// whole file over the network after it.
    pub(crate) stop: AtomicBool,
}

impl Run {
    pub(crate) fn new(total: u64) -> Arc<Self> {
        Arc::new(Self {
            done: AtomicU64::new(0),
            total,
            running: AtomicBool::new(true),
            stop: AtomicBool::new(false),
        })
    }

    pub(crate) fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// The run is over, whatever it managed.
    pub(crate) fn finish(&self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

/// What a reader asks its ingot about a precache in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Groups brought in so far.
    pub done: u64,
    /// Groups the run set out to bring.
    pub total: u64,
    /// Whether the thread is still at it: a run that finished, or died
    /// trying, is not.
    pub running: bool,
}

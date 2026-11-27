// SPDX-License-Identifier: GPL-3.0-or-later
//! Cutting an area of a disc into what an ORE stores: runs of zeros, runs of
//! padding as the seed that generates them, and the rest as MinCDC chunks.
//!
//! Padding is found as RVZ finds it, a 32 KiB block at a time, measured from
//! where the area's generator counts from: the disc for a GameCube disc and
//! outside Wii partitions, a partition's decrypted data within one.

use super::cdc;
use super::format::{BLOCK, RECORD_MAX, Run};
use super::fst::FileSystem;
use super::lfg::{Lfg, SEED_BYTES, SEED_SIZE};
use super::xbox::Filler;

use std::collections::{HashMap, VecDeque};

/// Zeros shorter than this stay in the chunks around them.
const ZERO_MIN: u64 = BLOCK;

enum Piece {
    /// A run of one byte.
    Repeat(u64, u8),
    Padding(u64, [u8; SEED_BYTES]),
    Data(usize, usize),
}

/// An area being cut, fed in order.
pub struct Segmenter {
    /// Where the area's generator counts from, at area offset 0.
    phase: u64,
    data: Vec<u8>,
    /// The run of one byte being counted, and its byte.
    run: u64,
    byte: u8,
    /// Whether to look for the GameCube and Wii padding generator.
    padding: bool,
    /// An Xbox disc's filler stream, looked for a sector at a time.
    xbox: Option<XboxStream>,
}

/// An Xbox disc's sector of filler.
const XBOX_SECTOR: usize = 2048;
/// How far ahead in the stream a sector is looked for: past a security
/// range, 4096 sectors the stream runs on through but the disc keeps.
const XBOX_WINDOW: usize = 1 << 13;

/// The filler stream of an Xbox disc, a window of it ahead.
struct XboxStream {
    /// The state after the window.
    next: Filler,
    /// The window's sectors: the state each starts from, and its key.
    ahead: VecDeque<(Filler, u64)>,
    /// The window's sectors by their first eight bytes, to their index.
    index: HashMap<u64, u64>,
    /// The stream index of the window's first sector.
    base: u64,
    /// The filler run so far: its state, its length, the index after it.
    run: Option<(Filler, u64, u64)>,
}

impl XboxStream {
    fn new(seed: u32) -> Self {
        let mut stream = Self {
            next: Filler::new(seed),
            ahead: VecDeque::new(),
            index: HashMap::new(),
            base: 0,
            run: None,
        };
        stream.refill();
        stream
    }

    fn sector(filler: &Filler) -> [u8; XBOX_SECTOR] {
        let mut sector = [0; XBOX_SECTOR];
        filler.clone().fill(0, &mut sector);
        sector
    }

    fn key(sector: &[u8]) -> u64 {
        u64::from_le_bytes(sector[..8].try_into().unwrap())
    }

    fn refill(&mut self) {
        while self.ahead.len() < XBOX_WINDOW {
            let at = self.base + self.ahead.len() as u64;
            let state = self.next.clone();
            let key = Self::key(&Self::sector(&state));
            self.index.insert(key, at);
            self.next.forward(XBOX_SECTOR as u64 / 2);
            self.ahead.push_back((state, key));
        }
    }

    /// Where in the window `sector` is, if it is there.
    fn find(&self, sector: &[u8]) -> Option<u64> {
        let &at = self.index.get(&Self::key(sector))?;
        (Self::sector(&self.ahead[(at - self.base) as usize].0) == sector).then_some(at)
    }

    /// Whether the window's sector `at` continues the run so far.
    fn continues(&self, at: u64) -> bool {
        matches!(self.run, Some((_, len, next)) if next == at && len + XBOX_SECTOR as u64 <= RECORD_MAX)
    }

    /// Take the window's sector `at` into the run, a new one unless it
    /// continues it, and the window past it.
    fn take(&mut self, at: u64, emit: &mut dyn FnMut(Run)) {
        if self.continues(at) {
            let (_, len, next) = self.run.as_mut().unwrap();
            *len += XBOX_SECTOR as u64;
            *next += 1;
        } else {
            self.settle(emit);
            let state = self.ahead[(at - self.base) as usize].0.clone();
            self.run = Some((state, XBOX_SECTOR as u64, at + 1));
        }
        for _ in self.base..=at {
            let (_, key) = self.ahead.pop_front().unwrap();
            self.index.remove(&key);
        }
        self.base = at + 1;
        self.refill();
    }

    /// Emit the run so far.
    fn settle(&mut self, emit: &mut dyn FnMut(Run)) {
        if let Some((state, len, _)) = self.run.take() {
            emit(Run::XboxFiller(len, state.to_bytes()));
        }
    }
}

impl Segmenter {
    pub fn new(phase: u64) -> Self {
        Self {
            phase,
            data: Vec::new(),
            run: 0,
            byte: 0,
            padding: true,
            xbox: None,
        }
    }

    /// A segmenter for an Xbox disc whose filler `seed` generated.
    pub fn xbox(seed: u32) -> Self {
        Self {
            xbox: Some(XboxStream::new(seed)),
            ..Self::without_padding(0)
        }
    }

    /// A segmenter for a disc no padding is generated on, which looks for
    /// none.
    pub fn without_padding(phase: u64) -> Self {
        Self {
            padding: false,
            ..Self::new(phase)
        }
    }

    /// Cut `bytes`, which lie at `offset` into the area and end on a block
    /// boundary unless the area ends with them, emitting what is settled.
    pub fn feed(
        &mut self,
        bytes: &[u8],
        offset: u64,
        file_system: Option<&FileSystem>,
        emit: &mut dyn FnMut(Run),
    ) {
        if self.xbox.is_none() {
            return self.feed_blocks(bytes, offset, file_system, emit);
        }
        // An Xbox disc's area starts on a sector, and every window ends on one
        for (i, sector) in bytes.chunks(XBOX_SECTOR).enumerate() {
            let at = offset + (i * XBOX_SECTOR) as u64;
            let stream = self.xbox.as_mut().unwrap();
            if let Some(found) = (sector.len() == XBOX_SECTOR)
                .then(|| stream.find(sector))
                .flatten()
            {
                // What came before the run is settled first
                if !stream.continues(found) {
                    self.settle_run(emit);
                    self.flush(true, emit);
                }
                self.xbox.as_mut().unwrap().take(found, emit);
                continue;
            }
            stream.settle(emit);
            self.feed_blocks(sector, at, file_system, emit);
        }
    }

    fn feed_blocks(
        &mut self,
        bytes: &[u8],
        offset: u64,
        file_system: Option<&FileSystem>,
        emit: &mut dyn FnMut(Run),
    ) {
        let mut position = 0usize;
        while position < bytes.len() {
            let at = self.phase + offset + position as u64;
            let length =
                ((at + 1).next_multiple_of(BLOCK) - at).min((bytes.len() - position) as u64);
            let block = &bytes[position..position + length as usize];
            for piece in pieces(
                block,
                at,
                offset + position as u64,
                file_system,
                self.padding,
            ) {
                match piece {
                    Piece::Repeat(n, byte) => {
                        if self.run > 0 && self.byte != byte {
                            self.settle_run(emit);
                        }
                        self.byte = byte;
                        self.run += n;
                    }
                    Piece::Padding(n, seed) => {
                        self.settle_run(emit);
                        self.flush(true, emit);
                        emit(Run::Padding(n, seed));
                    }
                    Piece::Data(start, end) => {
                        self.settle_run(emit);
                        self.data.extend_from_slice(&block[start..end]);
                    }
                }
            }
            position += length as usize;
        }
        self.flush(false, emit);
    }

    /// Emit what is left at the area's end.
    pub fn finish(&mut self, emit: &mut dyn FnMut(Run)) {
        if let Some(xbox) = &mut self.xbox {
            xbox.settle(emit);
        }
        self.settle_run(emit);
        self.flush(true, emit);
    }

    /// A long run as its length, a short one into the chunks around it.
    fn settle_run(&mut self, emit: &mut dyn FnMut(Run)) {
        if self.run >= ZERO_MIN {
            self.flush(true, emit);
            while self.run > 0 {
                let n = self.run.min(RECORD_MAX);
                emit(match self.byte {
                    0 => Run::Zero(n),
                    byte => Run::Fill(n, byte),
                });
                self.run -= n;
            }
        } else {
            self.data
                .resize(self.data.len() + self.run as usize, self.byte);
            self.run = 0;
        }
    }

    /// Emit the chunks of the data so far, all of them at the end of a run,
    /// otherwise those whose end the data to come cannot move.
    fn flush(&mut self, end: bool, emit: &mut dyn FnMut(Run)) {
        let mut at = 0;
        while at < self.data.len() && (end || self.data.len() - at > cdc::MAX) {
            let length = cdc::cut(&self.data[at..]);
            emit(Run::Chunk(&self.data[at..at + length]));
            at += length;
        }
        self.data.drain(..at);
    }
}

/// A block cut into zeros, padding and data, as RVZ cuts it. It lies at `at`
/// for the generator, and at `area_at` into the area, as the file system's
/// offsets count.
fn pieces(
    block: &[u8],
    at: u64,
    area_at: u64,
    file_system: Option<&FileSystem>,
    padding: bool,
) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut position = 0;
    while position < block.len() {
        // Zeros wherever they start; another byte filling the rest of the
        // block, as a ROM's padding does, so the padding generator's blocks
        // are looked at as before
        let byte = block[position];
        let run = block[position..].iter().take_while(|&&b| b == byte).count();
        if byte == 0 || position + run == block.len() {
            pieces.push(Piece::Repeat(run as u64, byte));
            position += run;
            continue;
        }
        let mut seed = [0; SEED_SIZE];
        let found = if padding {
            Lfg::seed(
                &block[position..],
                ((at + position as u64) % BLOCK) as usize,
                &mut seed,
            )
        } else {
            0
        };
        if found > SEED_BYTES {
            let mut bytes = [0; SEED_BYTES];
            for (out, word) in bytes.chunks_mut(4).zip(seed) {
                out.copy_from_slice(&word.to_be_bytes());
            }
            pieces.push(Piece::Padding(found as u64, bytes));
            position += found;
            continue;
        }
        // Data: up to the end of the file here if it ends in this block,
        // where padding may start, or else the rest of the block
        let end = file_system
            .and_then(|fs| fs.file_at(area_at + position as u64))
            .map(|(offset, size)| (offset + size - area_at) as usize)
            .filter(|&end| end > position && end < block.len())
            .unwrap_or(block.len());
        pieces.push(Piece::Data(position, end));
        position = end;
    }
    pieces
}

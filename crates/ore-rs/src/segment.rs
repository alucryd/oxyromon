//! Cutting an area of a disc into what an ORE stores: runs of zeros, runs of
//! padding as the seed that generates them, and the rest as MinCDC chunks.
//!
//! Padding is found as RVZ finds it, a 32 KiB block at a time, measured from
//! where the area's generator counts from: the disc for a GameCube disc and
//! outside Wii partitions, a partition's decrypted data within one.

use super::cdc;
use super::fst::FileSystem;
use super::lfg::{Lfg, SEED_BYTES, SEED_SIZE};

/// The block padding is generated in.
pub const BLOCK: u64 = 0x8000;
/// Zeros shorter than this stay in the chunks around them.
const ZERO_MIN: u64 = BLOCK;
/// No record covers more than this, as the spec bounds a chunk.
pub const RECORD_MAX: u64 = 16 << 20;

#[derive(Debug, PartialEq)]
pub enum Run<'a> {
    Zero(u64),
    /// Padding: its length and the seed, big endian.
    Padding(u64, [u8; SEED_BYTES]),
    Chunk(&'a [u8]),
}

enum Piece {
    Zero(u64),
    Padding(u64, [u8; SEED_BYTES]),
    Data(usize, usize),
}

/// An area being cut, fed in order.
pub struct Segmenter {
    /// Where the area's generator counts from, at area offset 0.
    phase: u64,
    data: Vec<u8>,
    zeros: u64,
    /// Whether to look for the GameCube and Wii padding generator.
    padding: bool,
}

impl Segmenter {
    pub fn new(phase: u64) -> Self {
        Self {
            phase,
            data: Vec::new(),
            zeros: 0,
            padding: true,
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
                    Piece::Zero(n) => self.zeros += n,
                    Piece::Padding(n, seed) => {
                        self.settle_zeros(emit);
                        self.flush(true, emit);
                        emit(Run::Padding(n, seed));
                    }
                    Piece::Data(start, end) => {
                        self.settle_zeros(emit);
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
        self.settle_zeros(emit);
        self.flush(true, emit);
    }

    fn settle_zeros(&mut self, emit: &mut dyn FnMut(Run)) {
        if self.zeros >= ZERO_MIN {
            self.flush(true, emit);
            while self.zeros > 0 {
                let n = self.zeros.min(RECORD_MAX);
                emit(Run::Zero(n));
                self.zeros -= n;
            }
        } else {
            self.data.resize(self.data.len() + self.zeros as usize, 0);
            self.zeros = 0;
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
        let zeros = block[position..].iter().take_while(|&&b| b == 0).count();
        if zeros > 0 {
            pieces.push(Piece::Zero(zeros as u64));
            position += zeros;
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

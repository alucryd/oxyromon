//! STR video sectors, coded losslessly: the `str` codec.
//!
//! A group is Form 1 sectors' data, 2048 bytes each, as a CD data track's
//! video plane holds them. The sectors of a frame are gathered, its MDEC
//! bitstream (BS v1, v2 or v3) parsed into its codes, and the codes coded
//! in context: each block first as the same as the block where it lies in
//! the frame before or not; a block that is not, its DC against the last of
//! its kind, then each code, whether it ends the block, its run and its
//! level, in the context of luma or chroma, where in the block it lands,
//! its block so far, and the level at that position in the frame before's
//! block. A frame is only coded as codes when they write back to it byte
//! for byte; any other bytes are coded a byte at a time, so any bytes go
//! through and come back: the model only decides how small.
//!
//! Integer arithmetic throughout; the range coder is `range.rs`'s.

use std::ops::Range;
use std::sync::LazyLock;

use super::range::{Bit, Coder, Decoder, Encoder};

pub const SECTOR: usize = 2048;
/// The standard sector header, `80010160`, and Final Fantasy VIII's, `SMJ\1`.
const MAGIC: [u8; 4] = 0x8001_0160u32.to_le_bytes();
const SMJ: &[u8; 4] = b"SMJ\x01";
const HEADERS: [usize; 2] = [32, 8];
/// Final Fantasy VII's camera data, before each frame's BS header.
const CAMERA: usize = 40;

/// Whether a Form 1 sector of this data goes in the video plane.
pub fn is_video(data: &[u8]) -> bool {
    data.starts_with(&MAGIC) || data.starts_with(SMJ)
}

/// Whether a video sector of this data starts a frame: a chunk is cut
/// before it.
pub fn starts_frame(data: &[u8]) -> bool {
    is_video(data) && data[4] == 0 && (data[..4] == *SMJ || data[5] == 0)
}

pub fn encode(data: &[u8]) -> Vec<u8> {
    let mut encoder = Encoder::new();
    let mut copy = data.to_vec();
    code(&mut encoder, &mut copy, true);
    encoder.finish()
}

/// Decode `coded` into `length` bytes. A coded stream cut short decodes to
/// bytes the group's CRC then refuses.
pub fn decode(coded: &[u8], length: usize) -> Vec<u8> {
    let mut out = vec![0; length];
    code(&mut Decoder::new(coded), &mut out, false);
    out
}

// The MPEG-1 AC codes STR uses: (code, its length, run, level), the sign
// bit after the code.
const AC: [(u32, u32, u32, u32); 111] = {
    const fn c(s: &[u8], run: u32, level: u32) -> (u32, u32, u32, u32) {
        let mut code = 0;
        let mut i = 0;
        while i < s.len() {
            code = code << 1 | (s[i] - b'0') as u32;
            i += 1;
        }
        (code, s.len() as u32, run, level)
    }
    [
        c(b"11", 0, 1),
        c(b"011", 1, 1),
        c(b"0100", 0, 2),
        c(b"0101", 2, 1),
        c(b"00101", 0, 3),
        c(b"00110", 4, 1),
        c(b"00111", 3, 1),
        c(b"000100", 7, 1),
        c(b"000101", 6, 1),
        c(b"000110", 1, 2),
        c(b"000111", 5, 1),
        c(b"0000100", 2, 2),
        c(b"0000101", 9, 1),
        c(b"0000110", 0, 4),
        c(b"0000111", 8, 1),
        c(b"00100000", 13, 1),
        c(b"00100001", 0, 6),
        c(b"00100010", 12, 1),
        c(b"00100011", 11, 1),
        c(b"00100100", 3, 2),
        c(b"00100101", 1, 3),
        c(b"00100110", 0, 5),
        c(b"00100111", 10, 1),
        c(b"0000001000", 16, 1),
        c(b"0000001001", 5, 2),
        c(b"0000001010", 0, 7),
        c(b"0000001011", 2, 3),
        c(b"0000001100", 1, 4),
        c(b"0000001101", 15, 1),
        c(b"0000001110", 14, 1),
        c(b"0000001111", 4, 2),
        c(b"000000010000", 0, 11),
        c(b"000000010001", 8, 2),
        c(b"000000010010", 4, 3),
        c(b"000000010011", 0, 10),
        c(b"000000010100", 2, 4),
        c(b"000000010101", 7, 2),
        c(b"000000010110", 21, 1),
        c(b"000000010111", 20, 1),
        c(b"000000011000", 0, 9),
        c(b"000000011001", 19, 1),
        c(b"000000011010", 18, 1),
        c(b"000000011011", 1, 5),
        c(b"000000011100", 3, 3),
        c(b"000000011101", 0, 8),
        c(b"000000011110", 6, 2),
        c(b"000000011111", 17, 1),
        c(b"0000000010000", 10, 2),
        c(b"0000000010001", 9, 2),
        c(b"0000000010010", 5, 3),
        c(b"0000000010011", 3, 4),
        c(b"0000000010100", 2, 5),
        c(b"0000000010101", 1, 7),
        c(b"0000000010110", 1, 6),
        c(b"0000000010111", 0, 15),
        c(b"0000000011000", 0, 14),
        c(b"0000000011001", 0, 13),
        c(b"0000000011010", 0, 12),
        c(b"0000000011011", 26, 1),
        c(b"0000000011100", 25, 1),
        c(b"0000000011101", 24, 1),
        c(b"0000000011110", 23, 1),
        c(b"0000000011111", 22, 1),
        c(b"00000000010000", 0, 31),
        c(b"00000000010001", 0, 30),
        c(b"00000000010010", 0, 29),
        c(b"00000000010011", 0, 28),
        c(b"00000000010100", 0, 27),
        c(b"00000000010101", 0, 26),
        c(b"00000000010110", 0, 25),
        c(b"00000000010111", 0, 24),
        c(b"00000000011000", 0, 23),
        c(b"00000000011001", 0, 22),
        c(b"00000000011010", 0, 21),
        c(b"00000000011011", 0, 20),
        c(b"00000000011100", 0, 19),
        c(b"00000000011101", 0, 18),
        c(b"00000000011110", 0, 17),
        c(b"00000000011111", 0, 16),
        c(b"000000000010000", 0, 40),
        c(b"000000000010001", 0, 39),
        c(b"000000000010010", 0, 38),
        c(b"000000000010011", 0, 37),
        c(b"000000000010100", 0, 36),
        c(b"000000000010101", 0, 35),
        c(b"000000000010110", 0, 34),
        c(b"000000000010111", 0, 33),
        c(b"000000000011000", 0, 32),
        c(b"000000000011001", 1, 14),
        c(b"000000000011010", 1, 13),
        c(b"000000000011011", 1, 12),
        c(b"000000000011100", 1, 11),
        c(b"000000000011101", 1, 10),
        c(b"000000000011110", 1, 9),
        c(b"000000000011111", 1, 8),
        c(b"0000000000010000", 1, 18),
        c(b"0000000000010001", 1, 17),
        c(b"0000000000010010", 1, 16),
        c(b"0000000000010011", 1, 15),
        c(b"0000000000010100", 6, 3),
        c(b"0000000000010101", 16, 2),
        c(b"0000000000010110", 15, 2),
        c(b"0000000000010111", 14, 2),
        c(b"0000000000011000", 13, 2),
        c(b"0000000000011001", 12, 2),
        c(b"0000000000011010", 11, 2),
        c(b"0000000000011011", 31, 1),
        c(b"0000000000011100", 30, 1),
        c(b"0000000000011101", 29, 1),
        c(b"0000000000011110", 28, 1),
        c(b"0000000000011111", 27, 1),
    ]
};
const ESCAPE: u32 = 0b000001;
const EOB: u32 = 0b10;

/// v3's DC codes, MPEG-1's: (code, length), the difference's size class
/// their index.
const DC_CHROMA: [(u32, u32); 9] = [
    (0b00, 2),
    (0b01, 2),
    (0b10, 2),
    (0b110, 3),
    (0b1110, 4),
    (0b11110, 5),
    (0b111110, 6),
    (0b1111110, 7),
    (0b11111110, 8),
];
const DC_LUMA: [(u32, u32); 9] = [
    (0b100, 3),
    (0b00, 2),
    (0b01, 2),
    (0b101, 3),
    (0b110, 3),
    (0b1110, 4),
    (0b11110, 5),
    (0b111110, 6),
    (0b1111110, 7),
];

/// The AC code each 16 bits of the stream start with, its index plus one.
static AC_BY_BITS: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let mut table = vec![0; 1 << 16];
    for (i, &(code, n, _, _)) in AC.iter().enumerate() {
        let from = (code << (16 - n)) as usize;
        table[from..from + (1 << (16 - n))].fill(i as u8 + 1);
    }
    table
});

/// The AC code of each run and level, its index plus one.
static AC_BY_VALUE: LazyLock<[[u8; 41]; 32]> = LazyLock::new(|| {
    let mut table = [[0; 41]; 32];
    for (i, &(_, _, run, level)) in AC.iter().enumerate() {
        table[run as usize][level as usize] = i as u8 + 1;
    }
    table
});

fn ac_index(run: u32, level: u32) -> Option<usize> {
    let i = *AC_BY_VALUE.get(run as usize)?.get(level as usize)?;
    (i != 0).then(|| i as usize - 1)
}

/// A frame's bitstream, as bytes in the order its bits are read: 16-bit
/// little-endian words, high bit first. Zeros past the end, to read 8
/// bytes at a time.
fn stream_order(bs: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = bs
        .as_chunks::<2>()
        .0
        .iter()
        .flat_map(|&[a, b]| [b, a])
        .collect();
    out.extend([0; 8]);
    out
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    bits: usize,
}

impl Reader<'_> {
    fn has(&self, n: u32) -> bool {
        self.at + n as usize <= self.bits
    }

    /// The next `n` bits, at most 32, zeros past the end.
    fn peek(&self, n: u32) -> u32 {
        let byte = self.at / 8;
        let word = u64::from_be_bytes(self.bytes[byte..byte + 8].try_into().unwrap());
        ((word << (self.at % 8)).checked_shr(64 - n).unwrap_or(0)) as u32
    }

    fn read(&mut self, n: u32) -> Option<u32> {
        self.has(n).then(|| {
            let v = self.peek(n);
            self.at += n as usize;
            v
        })
    }
}

#[derive(Default)]
struct Writer {
    bytes: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Writer {
    fn write(&mut self, value: u32, n: u32) {
        self.acc = self.acc << n | (value as u64 & ((1 << n) - 1));
        self.n += n;
        while self.n >= 8 {
            self.n -= 8;
            self.bytes.push((self.acc >> self.n) as u8);
        }
    }

    fn bits(&self) -> usize {
        self.bytes.len() * 8 + self.n as usize
    }

    /// The bitstream, `len` bytes of it, back in words.
    fn finish(mut self, len: usize) -> Vec<u8> {
        if self.n > 0 {
            self.bytes.push((self.acc << (8 - self.n)) as u8);
        }
        self.bytes.resize(len, 0);
        for word in self.bytes.as_chunks_mut::<2>().0 {
            word.swap(0, 1);
        }
        self.bytes
    }
}

/// An MDEC code of the bitstream, as it was coded.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Code {
    /// v2: the 10-bit DC; v3: the DC, in 4s, coded as the difference
    /// from the last of its kind.
    Dc(i16),
    Ac {
        run: u8,
        level: i16,
        escaped: bool,
    },
    Eob,
}

/// The codes of a frame's bitstream `bs`, its BS header past: its
/// macroblocks', block by block, Cr Cb Y1 Y2 Y3 Y4; and the bit they end.
fn parse(bs: &[u8], version: u16, macroblocks: usize) -> Option<(Vec<Code>, usize)> {
    let bytes = stream_order(bs);
    let mut r = Reader {
        bytes: &bytes,
        at: 64,
        bits: bs.len() * 8,
    };
    let mut codes = Vec::new();
    let mut previous = [0i32; 3];
    for _ in 0..macroblocks {
        for block in 0..6 {
            if version == 2 {
                let dc = r.read(10)?;
                codes.push(Code::Dc((((dc << 22) as i32) >> 22) as i16));
            } else {
                let (table, kind) = if block < 2 {
                    (&DC_CHROMA, block)
                } else {
                    (&DC_LUMA, 2)
                };
                let class = table
                    .iter()
                    .position(|&(code, n)| r.has(n) && r.peek(n) == code)?;
                r.at += table[class].1 as usize;
                let size = class as u32;
                let diff = if size == 0 {
                    0
                } else {
                    let v = r.read(size)? as i32;
                    if v >> (size - 1) == 0 {
                        v - (1 << size) + 1
                    } else {
                        v
                    }
                };
                previous[kind] += diff * 4;
                if !(-512..=511).contains(&previous[kind]) {
                    return None;
                }
                codes.push(Code::Dc((previous[kind] / 4) as i16));
            }
            let mut position = 0;
            loop {
                if !r.has(2) {
                    return None;
                }
                if r.peek(2) == EOB {
                    r.at += 2;
                    codes.push(Code::Eob);
                    break;
                }
                if !r.has(6) {
                    return None;
                }
                let (run, level, escaped) = if r.peek(6) == ESCAPE {
                    r.at += 6;
                    let run = r.read(6)?;
                    let level = ((r.read(10)? << 22) as i32) >> 22;
                    (run, level, true)
                } else {
                    let i = AC_BY_BITS[r.peek(16) as usize];
                    let &(_, n, run, level) = AC.get((i as usize).checked_sub(1)?)?;
                    if !r.has(n + 1) {
                        return None;
                    }
                    r.at += n as usize;
                    let sign = r.read(1)?;
                    (
                        run,
                        if sign == 1 {
                            -(level as i32)
                        } else {
                            level as i32
                        },
                        false,
                    )
                };
                position += run + 1;
                if position >= 64 {
                    return None;
                }
                codes.push(Code::Ac {
                    run: run as u8,
                    level: level as i16,
                    escaped,
                });
            }
        }
    }
    Some((codes, r.at))
}

/// Write `codes` as bits.
fn emit(codes: &[Code], version: u16, w: &mut Writer) {
    let mut block = 0;
    let mut previous = [0i32; 3];
    for &code in codes {
        match code {
            Code::Dc(v) if version == 2 => w.write(v as u32, 10),
            Code::Dc(v) => {
                let kind = [0, 1, 2, 2, 2, 2][block % 6];
                let diff = v as i32 - previous[kind];
                previous[kind] = v as i32;
                let table = if block % 6 < 2 { &DC_CHROMA } else { &DC_LUMA };
                let size = (32 - diff.unsigned_abs().leading_zeros()).min(8);
                let (code, n) = table[size as usize];
                w.write(code, n);
                if size > 0 {
                    let v = if diff < 0 {
                        diff + (1 << size) - 1
                    } else {
                        diff
                    };
                    w.write(v as u32, size);
                }
            }
            Code::Ac {
                run,
                level,
                escaped,
            } => match ac_index(run as u32, level.unsigned_abs() as u32) {
                Some(i) if !escaped => {
                    w.write(AC[i].0, AC[i].1);
                    w.write((level < 0) as u32, 1);
                }
                _ => {
                    w.write(ESCAPE, 6);
                    w.write(run as u32, 6);
                    w.write(level as u32, 10);
                }
            },
            Code::Eob => {
                w.write(EOB, 2);
                block += 1;
            }
        }
    }
}

/// The version a frame's bitstream is parsed as: v1 is v2 under another
/// number.
fn version(bs: &[u8]) -> Option<u16> {
    match u16::from_le_bytes([bs[6], bs[7]]) {
        1 | 2 => Some(2),
        3 => Some(3),
        _ => None,
    }
}

/// Whether a frame of `macroblocks` can be parsed from `bs`: even, and
/// long enough for that many blocks; what the decoder checks too.
fn parsable(bs: &[u8], macroblocks: usize) -> bool {
    bs.len() >= 8
        && bs.len().is_multiple_of(2)
        && macroblocks > 0
        // A block takes 4 bits at least, a v3 DC of 2 and its end
        && macroblocks * 6 * 4 <= bs.len() * 8
}

/// A frame's codes, and the bits past them, when they write back to its
/// bitstream byte for byte.
fn parse_exact(bs: &[u8], macroblocks: usize) -> Option<(Vec<Code>, Vec<u32>)> {
    if !parsable(bs, macroblocks) {
        return None;
    }
    let version = version(bs)?;
    let (codes, end) = parse(bs, version, macroblocks)?;
    let bytes = stream_order(bs);
    let mut r = Reader {
        bytes: &bytes,
        at: end,
        bits: bs.len() * 8,
    };
    let tail: Vec<u32> = (end..bs.len() * 8).map(|_| r.read(1).unwrap()).collect();
    let mut w = Writer {
        bytes: bytes[..8].to_vec(),
        ..Writer::default()
    };
    emit(&codes, version, &mut w);
    for &bit in &tail {
        w.write(bit, 1);
    }
    (w.finish(bs.len()) == bs).then_some((codes, tail))
}

/// A frame's codes, and where each block's lie.
#[derive(Default)]
struct Frame {
    codes: Vec<Code>,
    blocks: Vec<Range<usize>>,
}

impl Frame {
    fn new(codes: Vec<Code>) -> Self {
        let mut blocks = Vec::new();
        let mut from = 0;
        for (i, code) in codes.iter().enumerate() {
            if *code == Code::Eob {
                blocks.push(from..i + 1);
                from = i + 1;
            }
        }
        Self { codes, blocks }
    }

    fn block(&self, b: usize) -> &[Code] {
        self.blocks.get(b).map_or(&[], |r| &self.codes[r.clone()])
    }
}

/// Bytes coded in place, each a bit when it is the same as in the last
/// such bytes, else through a tree of its position.
struct Fields {
    same: Vec<Bit>,
    trees: Vec<[Bit; 256]>,
    last: Vec<u8>,
}

impl Fields {
    fn new(n: usize) -> Self {
        Self {
            same: vec![Bit::default(); n],
            trees: vec![[Bit::default(); 256]; n],
            last: vec![0; n],
        }
    }

    fn code(&mut self, coder: &mut impl Coder, bytes: &mut [u8]) {
        for (p, byte) in bytes.iter_mut().enumerate() {
            if coder.bit(&mut self.same[p], (*byte == self.last[p]) as u32) == 1 {
                *byte = self.last[p];
            } else {
                coder.byte(&mut self.trees[p], byte);
            }
            self.last[p] = *byte;
        }
    }
}

/// The models of a group.
struct Model {
    kind: [Bit; 4],
    count: [Bit; 64],
    headers: [Fields; 2],
    bs_header: Fields,
    raw: [Bit; 256],
    camera: Bit,
    parsed: Bit,
    padding: Bit,
    tail: [Bit; 11],
    tail_zero: Bit,
    same: [Bit; 4],
    dc: Vec<[Bit; 64]>,
    dc_sign: Vec<Bit>,
    eob: Vec<Bit>,
    run: Vec<[Bit; 64]>,
    level: Vec<[Bit; 64]>,
    sign: Bit,
    escaped: Bit,
}

impl Model {
    fn new() -> Self {
        Self {
            kind: [Bit::default(); 4],
            count: [Bit::default(); 64],
            headers: HEADERS.map(Fields::new),
            bs_header: Fields::new(8),
            raw: [Bit::default(); 256],
            camera: Bit::default(),
            parsed: Bit::default(),
            padding: Bit::default(),
            tail: [Bit::default(); 11],
            tail_zero: Bit::default(),
            same: [Bit::default(); 4],
            dc: vec![[Bit::default(); 64]; 3 * 8],
            dc_sign: vec![Bit::default(); 3 * 8],
            eob: vec![Bit::default(); 2 * 64 * 4 * 4 * 2],
            run: vec![[Bit::default(); 64]; 2 * 16 * 4],
            level: vec![[Bit::default(); 64]; 2 * 64 * 4 * 4],
            sign: Bit::default(),
            escaped: Bit::default(),
        }
    }

    fn raw(&mut self, coder: &mut impl Coder, bytes: &mut [u8]) {
        for byte in bytes {
            coder.byte(&mut self.raw, byte);
        }
    }
}

/// `v`, below 2^16, as its size class in unary, then its bits below the
/// top.
fn number(coder: &mut impl Coder, bits: &mut [Bit; 64], v: u32) -> u32 {
    let class = 32 - v.leading_zeros();
    let mut c = 0;
    while c < 16 && coder.bit(&mut bits[c as usize], (c < class) as u32) == 1 {
        c += 1;
    }
    if c == 0 {
        return 0;
    }
    let mut out = 1;
    for i in (0..c - 1).rev() {
        out = out << 1 | coder.bit(&mut bits[32 + c as usize], v >> i & 1);
    }
    out
}

fn bucket(v: u32) -> usize {
    match v {
        0 => 0,
        1 => 1,
        2..=3 => 2,
        _ => 3,
    }
}

/// Code a frame's `blocks` blocks of codes against `last`, the frame
/// before's: `given` when encoding, nothing when decoding. Returns them.
fn code_blocks(
    coder: &mut impl Coder,
    m: &mut Model,
    given: &Frame,
    blocks: usize,
    last: &Frame,
) -> Vec<Code> {
    let comparable = last.blocks.len() == blocks;
    let mut out = Vec::with_capacity(given.codes.len());
    let mut previous_dc = [0i32; 3];
    let mut last_dc_class = [0usize; 3];
    let mut left_same = 0;
    for b in 0..blocks {
        let this = given.block(b);
        let luma = (b % 6 >= 2) as usize;
        let kind = [0, 1, 2, 2, 2, 2][b % 6];
        let before = last.block(b);
        if comparable {
            let same = coder.bit(&mut m.same[luma * 2 + left_same], (this == before) as u32);
            left_same = same as usize;
            if same == 1 {
                if let Some(&Code::Dc(dc)) = before.first() {
                    previous_dc[kind] = dc as i32;
                }
                out.extend_from_slice(before);
                continue;
            }
        }
        let size = bucket(before.len().saturating_sub(2) as u32);
        // The block before's levels, by position, and whether it has one
        // past each
        let mut there = [0u32; 64];
        let mut at = 0;
        for code in before.iter().skip(1) {
            if let Code::Ac { run, level, .. } = code {
                at += *run as usize + 1;
                there[at.min(63)] = level.unsigned_abs() as u32;
            }
        }
        let mut later = [false; 65];
        for i in (0..64).rev() {
            later[i] = later[i + 1] || there[i] != 0;
        }
        // The DC against the last of its kind
        let dc = match this.first() {
            Some(&Code::Dc(v)) => v as i32,
            _ => 0,
        };
        let d = dc - previous_dc[kind];
        let ctx = kind * 8 + last_dc_class[kind];
        let magnitude = number(coder, &mut m.dc[ctx], d.unsigned_abs());
        let negative = magnitude != 0 && coder.bit(&mut m.dc_sign[ctx], (d < 0) as u32) == 1;
        let dc = previous_dc[kind]
            + if negative {
                -(magnitude as i32)
            } else {
                magnitude as i32
            };
        previous_dc[kind] = dc;
        last_dc_class[kind] = (32 - magnitude.leading_zeros()).min(7) as usize;
        out.push(Code::Dc(dc as i16));
        let (mut position, mut coded, mut last_level) = (0u32, 0u32, 0u32);
        for k in 1.. {
            let code = this.get(k).copied().unwrap_or(Code::Eob);
            // Past position 63 a block can only end
            let eob = position >= 63 || {
                let ctx = (((luma * 64 + position as usize) * 4 + bucket(coded)) * 4 + size) * 2
                    + later[position as usize + 1] as usize;
                coder.bit(&mut m.eob[ctx], (code == Code::Eob) as u32) == 1
            };
            if eob {
                out.push(Code::Eob);
                break;
            }
            let (run, level, escaped) = match code {
                Code::Ac {
                    run,
                    level,
                    escaped,
                } => (run as u32, level as i32, escaped),
                _ => (0, 0, false),
            };
            let near = (luma * 16 + position.min(15) as usize) * 4 + bucket(last_level);
            let run = number(coder, &mut m.run[near], run).min(62 - position);
            position += run + 1;
            let p = position as usize;
            let at = ((luma * 64 + p) * 4 + bucket(last_level)) * 4 + bucket(there[p]);
            let magnitude = number(coder, &mut m.level[at], level.unsigned_abs()).min(512);
            let negative = magnitude != 0 && coder.bit(&mut m.sign, (level < 0) as u32) == 1;
            let escaped = match ac_index(run, magnitude) {
                Some(_) => coder.bit(&mut m.escaped, escaped as u32) == 1,
                None => true,
            };
            out.push(Code::Ac {
                run: run as u8,
                level: if negative {
                    -(magnitude as i32)
                } else {
                    magnitude as i32
                } as i16,
                escaped,
            });
            coded += 1;
            last_level = magnitude;
        }
    }
    out
}

/// The bits of a frame's bitstream past its codes: the end code and
/// padding, mostly zeros.
fn code_tail(coder: &mut impl Coder, m: &mut Model, tail: &mut [u32]) {
    let head = tail.len().min(10);
    for (i, bit) in tail[..head].iter_mut().enumerate() {
        *bit = coder.bit(&mut m.tail[i], *bit);
    }
    let rest = &mut tail[head..];
    if rest.is_empty() {
        return;
    }
    let zero = rest.iter().all(|&b| b == 0);
    if coder.bit(&mut m.tail_zero, zero as u32) == 1 {
        rest.fill(0);
    } else {
        for bit in rest {
            *bit = coder.bit(&mut m.tail[10], *bit);
        }
    }
}

/// Code a frame of `macroblocks`, its bytes in place, against `last`.
fn code_frame(
    coder: &mut impl Coder,
    m: &mut Model,
    frame: &mut [u8],
    macroblocks: usize,
    encoding: bool,
    last: &mut Frame,
) -> Option<()> {
    let u16_at = |at: usize| u16::from_le_bytes([frame[at], frame[at + 1]]);
    let camera =
        encoding && frame.len() > CAMERA + 8 && u16_at(2) != 0x3800 && u16_at(CAMERA + 2) == 0x3800;
    let prefix = if coder.bit(&mut m.camera, camera as u32) == 1 {
        CAMERA
    } else {
        0
    };
    let (camera, bs) = frame.split_at_mut_checked(prefix)?;
    m.raw(coder, camera);
    let parsed = if encoding {
        parse_exact(bs, macroblocks)
    } else {
        None
    };
    if coder.bit(&mut m.parsed, parsed.is_some() as u32) == 0 {
        m.raw(coder, bs);
        return Some(());
    }
    let (given, mut tail) =
        parsed.map_or_else(Default::default, |(codes, tail)| (Frame::new(codes), tail));
    if bs.len() < 8 {
        return None;
    }
    m.bs_header.code(coder, &mut bs[..8]);
    let version = version(bs)?;
    if !parsable(bs, macroblocks) {
        return None;
    }
    let codes = code_blocks(coder, m, &given, macroblocks * 6, last);
    if encoding {
        code_tail(coder, m, &mut tail);
    } else {
        let mut w = Writer {
            bytes: stream_order(&bs[..8])[..8].to_vec(),
            ..Writer::default()
        };
        emit(&codes, version, &mut w);
        let mut tail = vec![0; (bs.len() * 8).checked_sub(w.bits())?];
        code_tail(coder, m, &mut tail);
        for bit in tail {
            w.write(bit, 1);
        }
        bs.copy_from_slice(&w.finish(bs.len()));
    }
    *last = Frame::new(codes);
    Some(())
}

/// What the sectors from the first of `sectors` hold, as the encoder
/// finds it: a frame of the standard header (1) or Final Fantasy VIII's
/// (2), and its sectors; or a sector of neither (0).
fn frame_at(sectors: &[[u8; SECTOR]]) -> (u32, usize) {
    let u16_at = |s: &[u8], at: usize| u16::from_le_bytes([s[at], s[at + 1]]);
    let u32_at = |s: &[u8], at: usize| u32::from_le_bytes(s[at..at + 4].try_into().unwrap());
    let s = &sectors[0];
    // Final Fantasy VIII: `SMJ\1`, the chunk, the chunks, the frame number
    if s.starts_with(SMJ) {
        let frame = u16_at(s, 6);
        let count = sectors
            .iter()
            .take_while(|t| t.starts_with(SMJ) && u16_at(&t[..], 6) == frame)
            .count();
        return (2, count);
    }
    let chunks = u16_at(s, 6) as usize;
    let used = u32_at(s, 12) as usize;
    let frame = s.starts_with(&MAGIC)
        && u16_at(s, 4) == 0
        && (1..=sectors.len()).contains(&chunks)
        && (8..=chunks * (SECTOR - HEADERS[0])).contains(&used)
        && (0..chunks).all(|c| {
            let t = &sectors[c];
            t.starts_with(&MAGIC) && u16_at(t, 4) as usize == c && u32_at(t, 8) == u32_at(s, 8)
        });
    if frame { (1, chunks) } else { (0, 1) }
}

/// Code `data`: encoding reads it, decoding writes it. `None` when what is
/// decoded cannot be, a corrupt stream.
fn code(coder: &mut impl Coder, data: &mut [u8], encoding: bool) -> Option<()> {
    let mut m = Model::new();
    let mut last = Frame::default();
    let (sectors, rest) = data.as_chunks_mut::<SECTOR>();
    let mut i = 0;
    while i < sectors.len() {
        let (kind, count) = if encoding {
            frame_at(&sectors[i..])
        } else {
            (0, 0)
        };
        let kind = coder.tree(&mut m.kind, 2, kind);
        if !(1..=2).contains(&kind) {
            m.raw(coder, &mut sectors[i]);
            i += 1;
            continue;
        }
        let count = number(coder, &mut m.count, count as u32) as usize;
        let frame = sectors.get_mut(i..i + count).filter(|f| !f.is_empty())?;
        let header = HEADERS[kind as usize - 1];
        for sector in frame.iter_mut() {
            m.headers[kind as usize - 1].code(coder, &mut sector[..header]);
        }
        let s = &frame[0];
        let mut payload: Vec<u8> = frame.iter().flat_map(|s| &s[header..]).copied().collect();
        let (used, macroblocks) = if kind == 1 {
            let u16_at = |at: usize| u16::from_le_bytes([s[at], s[at + 1]]) as usize;
            let used = u32::from_le_bytes(s[12..16].try_into().unwrap()) as usize;
            (
                used.min(payload.len()),
                u16_at(16).div_ceil(16) * u16_at(18).div_ceil(16),
            )
        } else {
            (payload.len(), 20 * 14)
        };
        let (bytes, padding) = payload.split_at_mut(used);
        code_frame(coder, &mut m, bytes, macroblocks, encoding, &mut last)?;
        let zero = padding.iter().all(|&b| b == 0);
        if coder.bit(&mut m.padding, zero as u32) == 1 {
            padding.fill(0);
        } else {
            m.raw(coder, padding);
        }
        for (sector, data) in frame.iter_mut().zip(payload.chunks(SECTOR - header)) {
            sector[header..].copy_from_slice(data);
        }
        i += count;
    }
    m.raw(coder, rest);
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(n: usize, seed: u64) -> Vec<u8> {
        let mut state = seed;
        (0..n)
            .map(|_| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                (state >> 56) as u8
            })
            .collect()
    }

    /// A frame of `macroblocks` as codes: a few levels a block, the odd
    /// escape, and every other block the same as `last`'s.
    fn codes(macroblocks: usize, seed: u64, last: &[Code]) -> Vec<Code> {
        let r = noise(macroblocks * 6 * 8, seed);
        let last = Frame::new(last.to_vec());
        let mut out = Vec::new();
        for b in 0..macroblocks * 6 {
            let r = &r[b * 8..b * 8 + 8];
            if b % 2 == 0 && !last.blocks.is_empty() {
                out.extend_from_slice(last.block(b));
                continue;
            }
            out.push(Code::Dc(r[0] as i16 / 4 - 30));
            for k in 0..(r[1] % 5) as usize {
                let escaped = r[2 + k] < 20;
                out.push(Code::Ac {
                    run: r[2 + k] % 3,
                    level: if escaped {
                        100
                    } else {
                        ((r[2 + k] % 3) as i16 - 1) | 1
                    },
                    escaped,
                });
            }
            out.push(Code::Eob);
        }
        out
    }

    /// Frames of 32x32 written as their sectors: v2 then v3.
    fn sectors() -> Vec<u8> {
        let mut out = Vec::new();
        let mut last = Vec::new();
        for (n, version) in [2u16, 2, 3, 3].into_iter().enumerate() {
            let codes = codes(4, n as u64 + 1, &last);
            let mut w = Writer::default();
            for byte in stream_order(&[0x10, 0, 0, 0x38, 1, 0, version as u8, 0])[..8].iter() {
                w.write(*byte as u32, 8);
            }
            emit(&codes, version, &mut w);
            w.write(0b0111111111, 10);
            let len = w.bits().div_ceil(32) * 4;
            let bs = w.finish(len);
            assert!(parse_exact(&bs, 4).is_some(), "frame {n}");
            let mut sector = vec![0; SECTOR];
            sector[..4].copy_from_slice(&MAGIC);
            sector[6] = 1;
            sector[8] = n as u8;
            sector[12..16].copy_from_slice(&(bs.len() as u32).to_le_bytes());
            sector[16] = 32;
            sector[18] = 32;
            sector[32..32 + bs.len()].copy_from_slice(&bs);
            out.extend_from_slice(&sector);
            last = codes;
        }
        out
    }

    #[test]
    fn frames_round_trip_and_shrink() {
        let data = sectors();
        let coded = encode(&data);
        assert!(coded.len() < 400, "{} of {}", coded.len(), data.len());
        assert_eq!(decode(&coded, data.len()), data);
    }

    #[test]
    fn anything_round_trips() {
        let mut frames = sectors();
        // A frame cut short, and one whose bitstream is noise
        frames.truncate(frames.len() - 100);
        frames[SECTOR + 40..2 * SECTOR].copy_from_slice(&noise(SECTOR - 40, 7));
        let mut smj = noise(3 * SECTOR + 5, 3);
        for s in smj.as_chunks_mut::<SECTOR>().0 {
            s[..4].copy_from_slice(SMJ);
            s[6..8].fill(0);
        }
        for data in [noise(2 * SECTOR + 99, 1), frames, smj] {
            assert_eq!(decode(&encode(&data), data.len()), data);
        }
        assert_eq!(decode(&encode(&[]), 0), Vec::<u8>::new());
        // A corrupt stream decodes to wrong bytes, without panicking
        decode(&noise(500, 5), 4 * SECTOR);
    }
}

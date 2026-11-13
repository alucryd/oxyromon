//! Lossless recoding of STR video frames for the ORE study.
//!
//! `str <DUMP>...`: each dump `cd str` wrote with an OUT directory, a
//! video sector's data each, 2048 bytes, in disc order. Sectors of the
//! standard 32-byte header are gathered into frames; each frame of BS v2
//! or v3 is parsed into its MDEC codes, written back to bits and compared
//! with the frame, and its codes costed under adaptive context models, as
//! a range coder would code them. A row a dump: frames, what parsed, and
//! sizes against Zstandard.

use std::path::PathBuf;

use anyhow::{Result, bail};
use rayon::prelude::*;

const SECTOR: usize = 2048;
const HEADER: usize = 32;

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
const ESCAPE: (u32, u32) = (0b000001, 6);
const EOB: (u32, u32) = (0b10, 2);

/// v3's DC codes, MPEG-1's: (code, length, bits of the difference), the
/// difference's size class its index, luma's class 0 the third code.
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

struct BitReader<'a> {
    data: &'a [u8],
    at: usize,
}

impl BitReader<'_> {
    /// Bit `i` of the stream: 16-bit little-endian words, high bit first.
    fn bit(&self, i: usize) -> Option<u32> {
        let byte = (i / 16) * 2 + 1 - (i / 8 % 2);
        let shift = 7 - i % 8;
        self.data.get(byte).map(|b| (*b as u32 >> shift) & 1)
    }

    fn peek(&self, n: u32) -> Option<u32> {
        (0..n as usize).try_fold(0, |v, i| Some(v << 1 | self.bit(self.at + i)?))
    }

    /// `n` bits, zeros past the end.
    fn peek_padded(&self, n: u32) -> u32 {
        (0..n as usize).fold(0, |v, i| v << 1 | self.bit(self.at + i).unwrap_or(0))
    }

    fn read(&mut self, n: u32) -> Option<u32> {
        let v = self.peek(n)?;
        self.at += n as usize;
        Some(v)
    }
}

#[derive(Default)]
struct BitWriter {
    bits: Vec<u8>,
}

impl BitWriter {
    fn write(&mut self, value: u32, n: u32) {
        for i in (0..n).rev() {
            self.bits.push((value >> i & 1) as u8);
        }
    }

    fn bytes(&self, len: usize) -> Vec<u8> {
        let mut out = vec![0; len];
        for (i, &b) in self.bits.iter().enumerate() {
            let byte = (i / 16) * 2 + 1 - (i / 8 % 2);
            if byte < len {
                out[byte] |= b << (7 - i % 8);
            }
        }
        out
    }
}

/// An MDEC code of the bitstream, as it was coded.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Code {
    /// v2: the 10-bit DC; v3: the DC, in 4s, coded as the difference
    /// from the last of its kind.
    Dc(i32),
    Ac {
        run: u32,
        level: i32,
        escaped: bool,
    },
    Eob,
}

/// A frame's codes: its macroblocks', block by block, Cr Cb Y1 Y2 Y3 Y4.
fn parse(frame: &[u8], version: u16, macroblocks: usize) -> Option<(Vec<Code>, usize)> {
    let mut r = BitReader {
        data: frame,
        at: 64,
    };
    let mut codes = Vec::new();
    let mut previous = [0i32; 3];
    for _ in 0..macroblocks {
        for block in 0..6 {
            if version == 2 {
                let dc = r.read(10)?;
                codes.push(Code::Dc(((dc << 22) as i32) >> 22));
            } else {
                let (table, kind) = if block < 2 {
                    (&DC_CHROMA, block)
                } else {
                    (&DC_LUMA, 2)
                };
                let class = table
                    .iter()
                    .position(|&(code, n)| r.peek(n) == Some(code))?;
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
                codes.push(Code::Dc(previous[kind] / 4));
            }
            let mut position = 0;
            loop {
                if r.peek(EOB.1)? == EOB.0 {
                    r.at += 2;
                    codes.push(Code::Eob);
                    break;
                }
                let code = if r.peek(ESCAPE.1)? == ESCAPE.0 {
                    r.at += 6;
                    let run = r.read(6)?;
                    let level = ((r.read(10)? << 22) as i32) >> 22;
                    Code::Ac {
                        run,
                        level,
                        escaped: true,
                    }
                } else {
                    let bits = r.peek_padded(16);
                    let &(_, n, run, level) = AC
                        .iter()
                        .find(|&&(code, n, _, _)| bits >> (16 - n) == code)?;
                    r.peek(n + 1)?;
                    r.at += n as usize;
                    let sign = r.read(1)?;
                    Code::Ac {
                        run,
                        level: if sign == 1 {
                            -(level as i32)
                        } else {
                            level as i32
                        },
                        escaped: false,
                    }
                };
                let Code::Ac { run, .. } = code else {
                    unreachable!()
                };
                position += run + 1;
                if position >= 64 {
                    return None;
                }
                codes.push(code);
            }
        }
    }
    Some((codes, r.at))
}

/// Write `codes` back as bits, then the frame's bits past them as they
/// were: what must give back the frame byte for byte.
fn emit(codes: &[Code], version: u16, frame: &[u8], end: usize) -> Vec<u8> {
    let mut w = BitWriter::default();
    let r = BitReader { data: frame, at: 0 };
    for i in 0..64 {
        w.write(r.bit(i).unwrap(), 1);
    }
    let mut block = 0;
    let mut previous = [0i32; 3];
    for &code in codes {
        match code {
            Code::Dc(v) if version == 2 => w.write(v as u32 & 0x3FF, 10),
            Code::Dc(v) => {
                let kind = [0, 1, 2, 2, 2, 2][block % 6];
                let diff = v - previous[kind];
                previous[kind] = v;
                let table = if block % 6 < 2 { &DC_CHROMA } else { &DC_LUMA };
                let size = 32 - diff.unsigned_abs().leading_zeros();
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
                escaped: true,
            } => {
                w.write(ESCAPE.0, 6);
                w.write(run, 6);
                w.write(level as u32 & 0x3FF, 10);
            }
            Code::Ac {
                run,
                level,
                escaped: false,
            } => {
                let &(code, n, _, _) = AC
                    .iter()
                    .find(|&&(_, _, r, l)| r == run && l == level.unsigned_abs())
                    .expect("parsed from the table");
                w.write(code, n);
                w.write((level < 0) as u32, 1);
            }
            Code::Eob => {
                w.write(EOB.0, 2);
                block += 1;
            }
        }
    }
    for i in end..frame.len() * 8 {
        w.write(r.bit(i).unwrap(), 1);
    }
    w.bytes(frame.len())
}

// Adaptive models, costed: what a range coder would spend.

#[derive(Clone, Copy)]
struct Bit(u16);

impl Default for Bit {
    fn default() -> Self {
        Self(1 << 15)
    }
}

#[derive(Default)]
struct Cost(f64);

impl Cost {
    fn bit(&mut self, p: &mut Bit, bit: bool) {
        let zero = (p.0 as f64 / 65536.0).clamp(1e-4, 1.0 - 1e-4);
        self.0 -= if bit { 1.0 - zero } else { zero }.log2();
        if bit {
            p.0 -= p.0 >> 5;
        } else {
            p.0 += (65535 - p.0) >> 5;
        }
    }

    /// `v` as its size class, in unary, then its bits below the top.
    fn number(&mut self, bits: &mut [Bit; 64], v: u32) {
        let class = 32 - v.leading_zeros();
        for i in 0..=class.min(16) {
            self.bit(&mut bits[i as usize], i < class);
        }
        for i in (0..class.saturating_sub(1)).rev() {
            self.bit(&mut bits[32 + (class as usize).min(31)], v >> i & 1 == 1);
        }
    }
}

/// The models of a group of frames.
struct Model {
    eob: Vec<Bit>,
    escaped: Vec<Bit>,
    run: Vec<[Bit; 64]>,
    level: Vec<[Bit; 64]>,
    sign: Bit,
    dc: Vec<[Bit; 64]>,
    dc_sign: Vec<Bit>,
    same: [Bit; 4],
}

impl Model {
    fn new() -> Self {
        Self {
            eob: vec![Bit::default(); 2 * 64 * 4 * 4 * 2],
            escaped: vec![Bit::default(); 2],
            run: vec![[Bit::default(); 64]; 2 * 16 * 4],
            level: vec![[Bit::default(); 64]; 2 * 64 * 4 * 4],
            sign: Bit::default(),
            dc: vec![[Bit::default(); 64]; 3 * 8],
            dc_sign: vec![Bit::default(); 3 * 8],
            same: [Bit::default(); 4],
        }
    }
}

fn bucket(v: u32) -> usize {
    match v {
        0 => 0,
        1 => 1,
        2..=3 => 2,
        _ => 3,
    }
}

/// A frame's codes, block by block.
fn blocks(codes: &[Code]) -> Vec<&[Code]> {
    let mut out = Vec::new();
    let mut from = 0;
    for (i, code) in codes.iter().enumerate() {
        if *code == Code::Eob {
            out.push(&codes[from..=i]);
            from = i + 1;
        }
    }
    out
}

/// What a frame's codes cost under the model, against `last`, the frame
/// before's blocks: each block first as the same as the block where it
/// lies in the frame before, or not; a block that is not, each code in the
/// context of its block's kind, where in the block it lands, its block so
/// far, and how many codes the block before held.
fn cost(model: &mut Model, codes: &[Code], last: &[&[Code]], cost: &mut Cost) {
    let this = blocks(codes);
    let comparable = last.len() == this.len();
    let mut previous_dc = [0i32; 3];
    let mut last_dc_class = [0usize; 3];
    let mut left_same = 0;
    for (b, block) in this.iter().enumerate() {
        let luma = (b % 6 >= 2) as usize;
        let kind = [0, 1, 2, 2, 2, 2][b % 6];
        let Code::Dc(dc) = block[0] else {
            unreachable!()
        };
        if comparable {
            let same = *block == last[b];
            cost.bit(&mut model.same[luma * 2 + left_same], same);
            left_same = same as usize;
            if same {
                previous_dc[kind] = dc;
                continue;
            }
        }
        let before = if comparable {
            bucket(last[b].len() as u32 - 2)
        } else {
            0
        };
        // The block before's levels, by position
        let mut there = [0u32; 64];
        if comparable {
            let mut at = 0;
            for code in &last[b][1..] {
                if let Code::Ac { run, level, .. } = code {
                    at += run + 1;
                    there[at.min(63) as usize] = level.unsigned_abs();
                }
            }
        }
        // Whether the block before has a level past each position
        let mut later = [false; 65];
        for i in (0..64).rev() {
            later[i] = later[i + 1] || there[i] != 0;
        }
        // The DC against the last of its kind
        let d = dc - previous_dc[kind];
        previous_dc[kind] = dc;
        let ctx = kind * 8 + last_dc_class[kind];
        cost.number(&mut model.dc[ctx], d.unsigned_abs());
        if d != 0 {
            cost.bit(&mut model.dc_sign[ctx], d < 0);
        }
        last_dc_class[kind] = (32 - d.unsigned_abs().leading_zeros()).min(7) as usize;
        let (mut position, mut coded, mut last_level) = (0u32, 0u32, 0u32);
        for &code in &block[1..] {
            let ctx = (((luma * 64 + position.min(63) as usize) * 4 + bucket(coded)) * 4 + before)
                * 2
                + later[(position + 1).min(64) as usize] as usize;
            cost.bit(&mut model.eob[ctx], code == Code::Eob);
            if let Code::Ac {
                run,
                level,
                escaped,
            } = code
            {
                let near = (luma * 16 + (position.min(15) as usize)) * 4 + bucket(last_level);
                cost.number(&mut model.run[near], run);
                position += run + 1;
                let p = position.min(63) as usize;
                let at = ((luma * 64 + p) * 4 + bucket(last_level)) * 4 + bucket(there[p]);
                cost.number(&mut model.level[at], level.unsigned_abs());
                if level != 0 {
                    cost.bit(&mut model.sign, level < 0);
                }
                let in_table = AC
                    .iter()
                    .any(|&(_, _, r, l)| r == run && l == level.unsigned_abs());
                if in_table {
                    cost.bit(&mut model.escaped[0], escaped);
                }
                coded += 1;
                last_level = level.unsigned_abs();
            }
        }
    }
}

/// A frame as sectors gave it: its bytes and where they came from.
struct Frame {
    data: Vec<u8>,
    /// Bytes before the BS header, kept as they are: Final Fantasy VII's
    /// 40 of camera data.
    prefix: usize,
    version: u16,
    macroblocks: usize,
    /// Its sectors' 32-byte headers.
    headers: Vec<u8>,
    /// What its last sector holds past the frame.
    padding: Vec<u8>,
}

/// The frames of a dump, and the sectors no frame of the standard header
/// took.
fn frames(dump: &[u8]) -> (Vec<Frame>, Vec<u8>) {
    let sectors: Vec<&[u8]> = dump
        .as_chunks::<SECTOR>()
        .0
        .iter()
        .map(|s| &s[..])
        .collect();
    let u16_at = |s: &[u8], at: usize| u16::from_le_bytes([s[at], s[at + 1]]);
    let u32_at = |s: &[u8], at: usize| u32::from_le_bytes(s[at..at + 4].try_into().unwrap());
    let (mut frames, mut other) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i < sectors.len() {
        let s = sectors[i];
        // Final Fantasy VIII: `SMJ\1`, the chunk, the chunks of the AV
        // frame, its two audio sectors first, and the frame number; 320x224
        if s.starts_with(b"SMJ\x01") {
            let frame = u16_at(s, 6);
            let mut j = i;
            while j < sectors.len()
                && sectors[j].starts_with(b"SMJ\x01")
                && u16_at(sectors[j], 6) == frame
            {
                j += 1;
            }
            let mut payload = Vec::new();
            let mut headers = Vec::new();
            for t in &sectors[i..j] {
                headers.extend_from_slice(&t[..8]);
                payload.extend_from_slice(&t[8..]);
            }
            frames.push(Frame {
                prefix: 0,
                version: u16_at(&payload, 6),
                macroblocks: 20 * 14,
                data: payload,
                headers,
                padding: Vec::new(),
            });
            i = j;
            continue;
        }
        let chunks = u16_at(s, 6) as usize;
        let standard = u32_at(s, 0) == 0x8001_0160 && u16_at(s, 4) == 0;
        let run_ok = standard
            && i + chunks <= sectors.len()
            && (0..chunks).all(|c| {
                let t = sectors[i + c];
                u32_at(t, 0) == 0x8001_0160
                    && u16_at(t, 4) as usize == c
                    && u32_at(t, 8) == u32_at(s, 8)
            });
        let used = u32_at(s, 12) as usize;
        if !run_ok || used > chunks * (SECTOR - HEADER) || used < 8 {
            other.extend_from_slice(s);
            i += 1;
            continue;
        }
        let mut payload = Vec::new();
        let mut headers = Vec::new();
        for c in 0..chunks {
            headers.extend_from_slice(&sectors[i + c][..HEADER]);
            payload.extend_from_slice(&sectors[i + c][HEADER..]);
        }
        let padding = payload.split_off(used);
        let (w, h) = (u16_at(s, 16) as usize, u16_at(s, 18) as usize);
        let prefix = if u16_at(&payload, 2) != 0x3800
            && payload.len() > 48
            && u16_at(&payload, 42) == 0x3800
        {
            40
        } else {
            0
        };
        frames.push(Frame {
            prefix,
            version: u16_at(&payload, prefix + 6),
            macroblocks: w.div_ceil(16) * h.div_ceil(16),
            data: payload,
            headers,
            padding,
        });
        i += chunks;
    }
    (frames, other)
}

fn zstd(data: &[u8]) -> u64 {
    zstd::bulk::compress(data, 19).unwrap().len() as u64
}

fn main() -> Result<()> {
    let dumps: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if dumps.is_empty() {
        bail!("usage: str <DUMP>...");
    }
    println!(
        "dump\tsectors\tframes\tv2\tv3\tparsed\texact\tbytes\tzstd\tframe_bytes\tframe_zstd\tframe_zstd_alone\tcodes_cost\theaders_zstd\tpadding_zstd\tpadding_nonzero\tunparsed_zstd\tother_zstd"
    );
    for dump in &dumps {
        let bytes = std::fs::read(dump)?;
        let name = dump.file_stem().unwrap_or_default().to_string_lossy();
        let (frames, other) = frames(&bytes);
        let mib = |data: &[u8]| -> u64 { data.par_chunks(1 << 20).map(zstd).sum() };
        // Groups of frames about 1 MiB, each costed on its own
        let mut groups: Vec<Vec<&Frame>> = vec![Vec::new()];
        let mut size = 0;
        for frame in &frames {
            if size > 1 << 20 {
                groups.push(Vec::new());
                size = 0;
            }
            size += frame.data.len();
            groups.last_mut().unwrap().push(frame);
        }
        let results: Vec<_> = groups
            .par_iter()
            .map(|group| {
                let mut model = Model::new();
                let mut c = Cost::default();
                let (mut parsed, mut exact, mut unparsed) = (0, 0, Vec::new());
                let mut alone = 0u64;
                let mut last: Vec<Code> = Vec::new();
                let mut parsed_bytes = Vec::new();
                for frame in group {
                    // v1 is v2 under another number
                    let version = if frame.version == 1 { 2 } else { frame.version };
                    let bs = &frame.data[frame.prefix..];
                    let codes = (version == 2 || version == 3)
                        .then(|| parse(bs, version, frame.macroblocks))
                        .flatten();
                    match codes {
                        Some((codes, end)) => {
                            parsed += 1;
                            if emit(&codes, version, bs, end) == bs {
                                exact += 1;
                            }
                            c.0 += frame.prefix as f64 * 8.0;
                            cost(&mut model, &codes, &blocks(&last), &mut c);
                            alone += zstd(&frame.data);
                            last = codes;
                            // The bits past the codes: padding bits and the end
                            // The bits past the codes: as they are, but a
                            // byte when they are the end code and zeros
                            let tail = &bs[(end + 10).div_ceil(8).min(bs.len())..];
                            c.0 += if tail.iter().all(|&b| b == 0) {
                                (end.div_ceil(8) * 8 - end + 10 + 8) as f64
                            } else {
                                (bs.len() * 8 - end) as f64
                            };
                            parsed_bytes.extend_from_slice(&frame.data);
                        }
                        None => unparsed.extend_from_slice(&frame.data),
                    }
                }
                (
                    parsed,
                    exact,
                    c.0 / 8.0,
                    zstd(&parsed_bytes),
                    zstd(&unparsed),
                    alone,
                )
            })
            .collect();
        let parsed: u64 = results.iter().map(|r| r.0).sum();
        let exact: u64 = results.iter().map(|r| r.1).sum();
        let codes_cost: f64 = results.iter().map(|r| r.2).sum();
        let frame_zstd: u64 = results.iter().map(|r| r.3).sum();
        let unparsed_zstd: u64 = results.iter().map(|r| r.4).sum();
        let alone: u64 = results.iter().map(|r| r.5).sum();
        let headers: Vec<u8> = frames
            .iter()
            .flat_map(|f| f.headers.iter().copied())
            .collect();
        let padding: Vec<u8> = frames
            .iter()
            .flat_map(|f| f.padding.iter().copied())
            .collect();
        let frame_bytes: usize = frames.iter().map(|f| f.data.len()).sum();
        let v = |n: u16| frames.iter().filter(|f| f.version == n).count();
        println!(
            "{name}\t{}\t{}\t{}\t{}\t{parsed}\t{exact}\t{}\t{}\t{frame_bytes}\t{frame_zstd}\t{alone}\t{codes_cost:.0}\t{}\t{}\t{}\t{unparsed_zstd}\t{}",
            bytes.len() / SECTOR,
            frames.len(),
            v(2),
            v(3),
            bytes.len(),
            mib(&bytes),
            mib(&headers),
            mib(&padding),
            padding.iter().filter(|&&b| b != 0).count(),
            mib(&other),
        );
    }
    Ok(())
}

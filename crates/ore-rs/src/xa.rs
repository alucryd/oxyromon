//! XA ADPCM sectors, coded losslessly: the `xa` codec.
//!
//! A group is records of a sector each, its 4-byte subheader then its 2324
//! bytes of data. The coder keeps every byte, but codes them in the order
//! and the context an XA decoder gives them, run alongside: a sound group's
//! parameters against its channel's last; its copies of them as a bit when
//! they match; each 4-bit sample against what a normalised LMS predictor of
//! the decoded channel expects it to be, and how far off that predictor has
//! lately been, the right channel of a stereo sector predicted from the
//! left too. What is not XA ADPCM is coded byte by byte, so any bytes go
//! through and come back: the model only decides how small.
//!
//! Integer arithmetic throughout, so a decoder anywhere gives the same
//! bytes. The range coder is `range.rs`'s.

use std::collections::HashMap;

use super::range::{Bit, Coder, Decoder, Encoder};

/// A record: a sector's subheader, then its data.
pub const RECORD: usize = 4 + DATA;
const DATA: usize = 2324;
/// The sound groups of a sector, 128 bytes each, before its 20-byte tail.
const GROUPS: usize = 18;

/// Whether a sector of this subheader is XA ADPCM of 4 bits a sample, what
/// the codec models.
pub fn is_adpcm(subheader: &[u8]) -> bool {
    subheader[2] & 0x04 != 0 && subheader[3] & 0x30 == 0
}

pub fn encode(data: &[u8]) -> Vec<u8> {
    let mut encoder = Encoder::new();
    let mut copy = data.to_vec();
    code(&mut encoder, &mut copy);
    encoder.finish()
}

/// Decode `coded` into `length` bytes. A coded stream cut short decodes to
/// bytes the group's CRC then refuses.
pub fn decode(coded: &[u8], length: usize) -> Vec<u8> {
    let mut out = vec![0; length];
    code(&mut Decoder::new(coded), &mut out);
    out
}

/// XA's prediction filters, in 64ths.
const K0: [i32; 4] = [0, 60, 115, 98];
const K1: [i32; 4] = [0, 0, -52, -55];

/// The predictor's own samples, and the other channel's about this one.
const ORDER: usize = 8;
const CROSS: usize = 3;
const INPUTS: usize = ORDER + CROSS;
/// The LMS step, 0.1 in 16 bits, and the least energy it is normalised by.
const MU: i64 = 6554;
const ENERGY_MIN: i64 = 1 << 20;
const WEIGHT_MAX: i64 = 1 << 20;
/// The nibble's context: where the predictor puts it, in half steps, and
/// how far off it has lately been.
const EXPECTED: usize = 32;
const CONFIDENCE: usize = 6;

/// A channel of a stream being decoded: XA's state and the predictor's.
#[derive(Clone)]
struct Channel {
    old: i32,
    older: i32,
    last_param: u8,
    /// The last samples, newest first, then the other channel's.
    inputs: [i32; INPUTS],
    /// In 16 bits.
    weights: [i64; INPUTS],
    /// How far off the predictor has been, in 256ths of a step, smoothed.
    error: i64,
}

impl Default for Channel {
    fn default() -> Self {
        Self {
            old: 0,
            older: 0,
            last_param: 0,
            inputs: [0; INPUTS],
            weights: [0; INPUTS],
            error: 2 << 8,
        }
    }
}

impl Channel {
    fn predict(&self) -> i64 {
        let dot: i64 = self
            .inputs
            .iter()
            .zip(&self.weights)
            .map(|(&x, &w)| x as i64 * w)
            .sum();
        (dot >> 16).clamp(-1 << 16, 1 << 16)
    }

    /// Learn from `sample`, which the predictor put at `predicted`, and
    /// take it into the history.
    fn push(&mut self, sample: i32, predicted: i64) {
        let energy = self
            .inputs
            .iter()
            .map(|&x| x as i64 * x as i64)
            .sum::<i64>()
            .max(ENERGY_MIN);
        let factor = ((MU * (sample as i64 - predicted)) << 16) / energy;
        for (w, &x) in self.weights.iter_mut().zip(&self.inputs) {
            // Rounded towards zero, so weights do not drift
            *w = (*w + factor * x as i64 / (1 << 16)).clamp(-WEIGHT_MAX, WEIGHT_MAX);
        }
        self.inputs.copy_within(0..ORDER - 1, 1);
        self.inputs[0] = sample;
        self.older = self.old;
        self.old = sample;
    }
}

struct Model {
    subheader: [[Bit; 256]; 4],
    params: Vec<[Bit; 256]>,
    copies: Bit,
    raw: [Bit; 256],
    nibbles: Vec<[Bit; 16]>,
    tail: Bit,
}

impl Model {
    fn new() -> Self {
        Self {
            subheader: [[Bit::default(); 256]; 4],
            params: vec![[Bit::default(); 256]; 256],
            copies: Bit::default(),
            raw: [Bit::default(); 256],
            nibbles: vec![[Bit::default(); 16]; EXPECTED * CONFIDENCE],
            tail: Bit::default(),
        }
    }
}

/// Code `data`: encoding reads it, decoding writes it.
fn code(coder: &mut impl Coder, data: &mut [u8]) {
    let mut model = Model::new();
    // A stream's channels, by file and channel number, left and right
    let mut channels: HashMap<u32, Channel> = HashMap::new();
    let (records, rest) = data.as_chunks_mut::<RECORD>();
    for record in records {
        let (subheader, sector) = record.split_at_mut(4);
        for (nodes, byte) in model.subheader.iter_mut().zip(subheader.iter_mut()) {
            coder.byte(nodes, byte);
        }
        if !is_adpcm(subheader) {
            for byte in sector.iter_mut() {
                coder.byte(&mut model.raw, byte);
            }
            continue;
        }
        let stereo = subheader[3] & 3 == 1;
        let stream = (subheader[0] as u32) << 16 | (subheader[1] as u32) << 8;
        let key = |unit: usize| stream | (stereo && unit & 1 == 1) as u32;
        for group in sector[..GROUPS * 128].as_chunks_mut::<128>().0 {
            for unit in 0..8 {
                let channel = channels.entry(key(unit)).or_default();
                coder.byte(
                    &mut model.params[channel.last_param as usize],
                    &mut group[4 + unit],
                );
                channel.last_param = group[4 + unit];
            }
            let same = group[..4] == group[4..8] && group[12..16] == group[8..12];
            if coder.bit(&mut model.copies, same as u32) == 1 {
                group.copy_within(4..8, 0);
                group.copy_within(8..12, 12);
            } else {
                for i in (0..4).chain(12..16) {
                    coder.byte(&mut model.raw, &mut group[i]);
                }
            }
            let mut left = [0; 28];
            for unit in 0..8 {
                let channel = channels.get_mut(&key(unit)).expect("its parameter made it");
                let param = group[4 + unit];
                // Shifts past 12 the hardware takes as 9
                let shift = match param & 15 {
                    s @ 0..=12 => s as u32,
                    _ => 9,
                };
                let filter = (param >> 4 & 3) as usize;
                let right = stereo && unit & 1 == 1;
                for j in 0..28 {
                    if right {
                        for (d, input) in channel.inputs[ORDER..].iter_mut().enumerate() {
                            *input = left[(j + d).saturating_sub(1).min(27)];
                        }
                    }
                    let pred = (channel.old * K0[filter] + channel.older * K1[filter] + 32) >> 6;
                    let predicted = channel.predict();
                    // Where the predictor puts the nibble, in 256ths of a step
                    let expected = (predicted - pred as i64) << shift >> 4;
                    let half_steps = ((expected + 64) >> 7).clamp(-16, 15) + 16;
                    let confidence = (channel.error >> 7).min(CONFIDENCE as i64 - 1);
                    let context = half_steps as usize * CONFIDENCE + confidence as usize;
                    let (at, high) = (16 + j * 4 + unit / 2, (unit & 1) * 4);
                    let nibble = coder.tree(
                        &mut model.nibbles[context],
                        4,
                        (group[at] >> high & 15) as u32,
                    );
                    group[at] = group[at] & !(15 << high) | (nibble as u8) << high;
                    let signed = (nibble as i32) << 28 >> 28;
                    let sample = (((signed << 12) >> shift) + pred).clamp(-32768, 32767);
                    let off = ((signed as i64) << 8) - expected;
                    channel.error = (channel.error * 9 + off.abs().min(16 << 8)) / 10;
                    channel.push(sample, predicted);
                    if !stereo || unit & 1 == 0 {
                        left[j] = sample;
                    }
                }
            }
        }
        let tail = &mut sector[GROUPS * 128..];
        let zero = tail.iter().all(|&b| b == 0);
        if coder.bit(&mut model.tail, zero as u32) == 1 {
            tail.fill(0);
        } else {
            for byte in tail.iter_mut() {
                coder.byte(&mut model.raw, byte);
            }
        }
    }
    for byte in rest {
        coder.byte(&mut model.raw, byte);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mono and a stereo sector of a tone, XA encoded the simple way.
    fn sectors() -> Vec<u8> {
        let mut out = Vec::new();
        for (channel, coding) in [(0u8, 0u8), (1, 1)] {
            out.extend_from_slice(&[1, channel, 0x64, coding]);
            let mut sector = [0u8; DATA];
            let (mut old, mut older) = ([0i32; 2], [0i32; 2]);
            let mut n = 0;
            for group in sector[..GROUPS * 128].as_chunks_mut::<128>().0 {
                for unit in 0..8 {
                    let side = (coding == 1 && unit & 1 == 1) as usize;
                    let (shift, filter) = (8, 1);
                    group[4 + unit] = (filter << 4 | shift) as u8;
                    for j in 0..28 {
                        let x = ((n as f64 * 0.05).sin() * 8000.0) as i32 + side as i32 * 300;
                        n += 1;
                        let pred = (old[side] * K0[filter] + older[side] * K1[filter] + 32) >> 6;
                        let t = ((x - pred) >> (12 - shift)).clamp(-8, 7);
                        let at = 16 + j * 4 + unit / 2;
                        group[at] |= ((t & 15) as u8) << ((unit & 1) * 4);
                        let s = (((t << 12) >> shift) + pred).clamp(-32768, 32767);
                        older[side] = old[side];
                        old[side] = s;
                    }
                }
                group.copy_within(4..8, 0);
                group.copy_within(8..12, 12);
            }
            out.extend_from_slice(&sector);
        }
        out
    }

    #[test]
    fn xa_round_trips_and_shrinks() {
        let data = sectors();
        let coded = encode(&data);
        // 1,571 bytes: the copies cost a bit a sound group, not 8 bytes
        assert!(coded.len() < 1600, "{} of {}", coded.len(), data.len());
        assert_eq!(decode(&coded, data.len()), data);
    }

    #[test]
    fn anything_round_trips() {
        let mut state = 1u64;
        let noise: Vec<u8> = (0..RECORD * 2 + 99)
            .map(|_| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                (state >> 56) as u8
            })
            .collect();
        // Noise as XA, as something else, and a record cut short
        let mut data = noise.clone();
        data[2] = 0x64;
        data[3] = 0x01;
        for data in [noise, data] {
            assert_eq!(decode(&encode(&data), data.len()), data);
        }
        assert_eq!(decode(&encode(&[]), 0), Vec::<u8>::new());
    }
}

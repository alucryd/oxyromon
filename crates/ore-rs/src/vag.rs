//! SPU ADPCM, PlayStation 2 `VAGp` audio, coded losslessly: the `vag`
//! codec.
//!
//! A group is 16-byte blocks: a block's shift and filter, its flags, then
//! 28 samples of 4 bits, low nibble first. The coder keeps every byte, but
//! codes each block's parameters against the last's and each sample
//! against what `xa`'s LMS predictor of the decoded stream expects of it,
//! a SPU decoder run alongside. A block that opens with `VAGp` starts a
//! stream: its 48-byte header is coded as bytes and the decoder starts
//! again. What is not SPU ADPCM codes as some, so any bytes go through and
//! come back: the model only decides how small.

use super::range::{Bit, Coder, Decoder, Encoder};
use super::xa::{CONFIDENCE, Channel, EXPECTED};

const BLOCK: usize = 16;
const HEADER: usize = 48;

/// The SPU's prediction filters, in 64ths: XA's and a fifth.
const K0: [i32; 5] = [0, 60, 115, 98, 122];
const K1: [i32; 5] = [0, 0, -52, -55, -60];

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

struct Model {
    header: Bit,
    params: Vec<[Bit; 256]>,
    flags: Vec<[Bit; 256]>,
    raw: [Bit; 256],
    nibbles: Vec<[Bit; 16]>,
}

/// Code `data`: encoding reads it, decoding writes it.
fn code(coder: &mut impl Coder, data: &mut [u8]) {
    let mut model = Model {
        header: Bit::default(),
        params: vec![[Bit::default(); 256]; 256],
        flags: vec![[Bit::default(); 256]; 256],
        raw: [Bit::default(); 256],
        nibbles: vec![[Bit::default(); 16]; EXPECTED * CONFIDENCE],
    };
    let mut channel = Channel::default();
    let (mut param, mut flags) = (0u8, 0u8);
    let mut at = 0;
    while at + BLOCK <= data.len() {
        if at + HEADER <= data.len() {
            let header = data[at..at + 4] == *b"VAGp";
            if coder.bit(&mut model.header, header as u32) == 1 {
                for byte in &mut data[at..at + HEADER] {
                    coder.byte(&mut model.raw, byte);
                }
                data[at..at + 4].copy_from_slice(b"VAGp");
                channel = Channel::default();
                (param, flags) = (0, 0);
                at += HEADER;
                continue;
            }
        }
        let block = &mut data[at..at + BLOCK];
        coder.byte(&mut model.params[param as usize], &mut block[0]);
        coder.byte(&mut model.flags[flags as usize], &mut block[1]);
        (param, flags) = (block[0], block[1]);
        // Shifts past 12 the hardware takes as 9; filters past 4 model as 0
        let shift = match param & 15 {
            s @ 0..=12 => s as u32,
            _ => 9,
        };
        let filter = match (param >> 4) as usize {
            f @ 0..=4 => f,
            _ => 0,
        };
        for j in 0..28 {
            let pred = (channel.old * K0[filter] + channel.older * K1[filter] + 32) >> 6;
            let predicted = channel.predict();
            // Where the predictor puts the nibble, in 256ths of a step
            let expected = (predicted - pred as i64) << shift >> 4;
            let half_steps = ((expected + 64) >> 7).clamp(-16, 15) + 16;
            let confidence = (channel.error >> 7).min(CONFIDENCE as i64 - 1);
            let context = half_steps as usize * CONFIDENCE + confidence as usize;
            let (byte, high) = (2 + j / 2, (j & 1) * 4);
            let nibble = coder.tree(
                &mut model.nibbles[context],
                4,
                (block[byte] >> high & 15) as u32,
            );
            block[byte] = block[byte] & !(15 << high) | (nibble as u8) << high;
            let signed = (nibble as i32) << 28 >> 28;
            let sample = (((signed << 12) >> shift) + pred).clamp(-32768, 32767);
            let off = ((signed as i64) << 8) - expected;
            channel.error = (channel.error * 9 + off.abs().min(16 << 8)) / 10;
            channel.push(sample, predicted);
        }
        at += BLOCK;
    }
    for byte in &mut data[at..] {
        coder.byte(&mut model.raw, byte);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A VAG of a tone: its header, then blocks SPU encoded the simple way.
    pub(crate) fn tone(blocks: usize) -> Vec<u8> {
        let mut out = b"VAGp".to_vec();
        out.extend_from_slice(&32u32.to_be_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&((blocks * BLOCK) as u32).to_be_bytes());
        out.extend_from_slice(&44_100u32.to_be_bytes());
        out.resize(HEADER, 0);
        let (mut old, mut older, mut n) = (0i32, 0i32, 0);
        for b in 0..blocks {
            let (shift, filter) = (8, 2);
            let mut block = [0u8; BLOCK];
            block[0] = (filter << 4 | shift) as u8;
            block[1] = if b + 1 == blocks { 1 } else { 0 };
            for j in 0..28 {
                let x = ((n as f64 * 0.03).sin() * 9000.0) as i32;
                n += 1;
                let pred = (old * K0[filter] + older * K1[filter] + 32) >> 6;
                let t = ((x - pred) >> (12 - shift)).clamp(-8, 7);
                block[2 + j / 2] |= ((t & 15) as u8) << ((j & 1) * 4);
                let s = (((t << 12) >> shift) + pred).clamp(-32768, 32767);
                older = old;
                old = s;
            }
            out.extend_from_slice(&block);
        }
        out
    }

    #[test]
    fn vag_round_trips_and_shrinks() {
        let mut data = tone(2000);
        data.extend(tone(500));
        let coded = encode(&data);
        let zstd = zstd::bulk::compress(&data, 19).unwrap();
        // A sine repeats, which Zstandard finds too: real VAGs are measured
        assert!(
            coded.len() < zstd.len(),
            "{} against {}",
            coded.len(),
            zstd.len()
        );
        assert_eq!(decode(&coded, data.len()), data);
    }

    #[test]
    fn anything_round_trips() {
        let mut state = 1u64;
        let noise: Vec<u8> = (0..4099)
            .map(|_| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                (state >> 56) as u8
            })
            .collect();
        // Noise, noise with a header in it, and a header cut short
        let mut headed = noise.clone();
        headed[32..36].copy_from_slice(b"VAGp");
        let mut short = noise[..40].to_vec();
        short[..4].copy_from_slice(b"VAGp");
        for data in [noise, headed, short, Vec::new()] {
            assert_eq!(decode(&encode(&data), data.len()), data);
        }
    }
}

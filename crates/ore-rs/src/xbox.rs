// SPDX-License-Identifier: MIT OR Apache-2.0
//! The filler generator of Xbox discs mastered before version 4831 of the
//! layout tool, as xboxdevwiki's XDVDFS page describes it: a 32-bit state
//! stepped modulo a prime, two bytes a step.

/// What the state steps modulo, a prime.
const PRIME: u64 = 0xFFFF_FFFB;
/// The multipliers a seed picks from.
const MULTIPLIERS: [u32; 8] = [
    0x52F6_90D5,
    0x534D_7DDE,
    0x5B71_A70F,
    0x6679_3320,
    0x9B7E_5ED5,
    0xA465_265E,
    0xA53F_1D11,
    0xB154_430F,
];
/// A record's state: its mask, multiplier and counter, big endian.
pub const STATE_BYTES: usize = 12;

#[derive(Clone, Debug, PartialEq)]
pub struct Filler {
    a: u32,
    b: u32,
    c: u32,
}

impl Filler {
    /// The stream the layout tool started for `seed`.
    pub fn new(seed: u32) -> Self {
        let mut filler = Self {
            a: 0,
            b: MULTIPLIERS[(seed & 7) as usize],
            c: seed,
        };
        filler.a = filler.step();
        filler
    }

    pub fn from_bytes(bytes: &[u8; STATE_BYTES]) -> Self {
        let word = |i: usize| u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap());
        Self {
            a: word(0),
            b: word(4),
            c: word(8),
        }
    }

    pub fn to_bytes(&self) -> [u8; STATE_BYTES] {
        let mut bytes = [0; STATE_BYTES];
        bytes[..4].copy_from_slice(&self.a.to_be_bytes());
        bytes[4..8].copy_from_slice(&self.b.to_be_bytes());
        bytes[8..].copy_from_slice(&self.c.to_be_bytes());
        bytes
    }

    #[inline]
    fn step(&mut self) -> u32 {
        self.c = ((self.c as u64 + 1) * self.b as u64 % PRIME) as u32;
        self.c ^ self.a
    }

    /// The next two bytes of the stream, little endian.
    #[inline]
    pub fn word(&mut self) -> [u8; 2] {
        ((self.step() >> 8) as u16).to_le_bytes()
    }

    /// Skip `steps` words, in time logarithmic in them: a step is the
    /// affine map c ↦ b·c + b modulo the prime, composed by squaring.
    pub fn forward(&mut self, mut steps: u64) {
        let b = self.b as u64 % PRIME;
        // The map so far, x ↦ m·x + k, and the one to square
        let (mut m, mut k, mut pm, mut pk) = (1u64, 0u64, b, b);
        while steps > 0 {
            if steps & 1 == 1 {
                (m, k) = (pm * m % PRIME, (pm * k + pk) % PRIME);
            }
            (pm, pk) = (pm * pm % PRIME, (pm * pk + pk) % PRIME);
            steps >>= 1;
        }
        self.c = ((m * self.c as u64 + k) % PRIME) as u32;
    }

    /// Fill `out` from byte `at` of the stream.
    pub fn fill(&mut self, at: u64, out: &mut [u8]) {
        self.forward(at / 2);
        let mut out = out;
        if at % 2 == 1 && !out.is_empty() {
            out[0] = self.word()[1];
            out = &mut out[1..];
        }
        let (pairs, rest) = out.as_chunks_mut::<2>();
        for pair in pairs {
            *pair = self.word();
        }
        if let [last] = rest {
            *last = self.word()[0];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_skips_as_stepping_does() {
        let mut stepped = Filler::new(0xA122_0DB0);
        let mut skipped = stepped.clone();
        for _ in 0..12345 {
            stepped.step();
        }
        skipped.forward(12345);
        assert_eq!(stepped, skipped);
    }

    #[test]
    fn fill_from_an_odd_byte_is_the_stream_shifted() {
        let mut whole = vec![0; 64];
        Filler::new(5).fill(0, &mut whole);
        let mut part = vec![0; 37];
        Filler::new(5).fill(7, &mut part);
        assert_eq!(part, whole[7..44]);
    }
}

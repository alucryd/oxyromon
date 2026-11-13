//! The range coder of the modelling codecs, `xa` and `str`: LZMA's, its
//! probabilities 16-bit, each the mean of a fast and a slow estimate. One
//! [`Coder`] trait serves both ways, so a codec's model is written once.

#[derive(Clone, Copy)]
pub struct Bit {
    fast: u16,
    slow: u16,
}

impl Default for Bit {
    fn default() -> Self {
        Self {
            fast: 1 << 15,
            slow: 1 << 15,
        }
    }
}

impl Bit {
    /// The probability of a 0, in 16 bits, never 0 nor 1.
    fn p(&self) -> u32 {
        ((self.fast as u32 + self.slow as u32) >> 1).clamp(32, 65536 - 32)
    }

    fn update(&mut self, bit: u32) {
        if bit == 0 {
            self.fast += (65535 - self.fast) >> 4;
            self.slow += (65535 - self.slow) >> 7;
        } else {
            self.fast -= self.fast >> 4;
            self.slow -= self.slow >> 7;
        }
    }
}

/// The coder both ways: encoding codes the value it is given, decoding
/// ignores it and gives the value decoded, so one model serves both.
pub trait Coder {
    fn bit(&mut self, p: &mut Bit, bit: u32) -> u32;

    /// `value`'s low `bits` bits through a binary tree of `nodes`.
    fn tree(&mut self, nodes: &mut [Bit], bits: u32, value: u32) -> u32 {
        let mut node = 1;
        for i in (0..bits).rev() {
            node = node << 1 | self.bit(&mut nodes[node as usize], value >> i & 1);
        }
        node - (1 << bits)
    }

    fn byte(&mut self, nodes: &mut [Bit; 256], byte: &mut u8) {
        *byte = self.tree(nodes, 8, *byte as u32) as u8;
    }
}

pub struct Encoder {
    low: u64,
    range: u32,
    cache: u8,
    pending: u64,
    out: Vec<u8>,
}

impl Encoder {
    pub fn new() -> Self {
        Self {
            low: 0,
            range: u32::MAX,
            cache: 0,
            pending: 1,
            out: Vec::new(),
        }
    }

    fn shift_low(&mut self) {
        if self.low < 0xFF00_0000 || self.low > 0xFFFF_FFFF {
            let carry = (self.low >> 32) as u8;
            let mut byte = self.cache;
            loop {
                self.out.push(byte.wrapping_add(carry));
                byte = 0xFF;
                self.pending -= 1;
                if self.pending == 0 {
                    break;
                }
            }
            self.cache = (self.low >> 24) as u8;
        }
        self.pending += 1;
        self.low = (self.low & 0x00FF_FFFF) << 8;
    }

    pub fn finish(mut self) -> Vec<u8> {
        for _ in 0..5 {
            self.shift_low();
        }
        self.out
    }
}

impl Coder for Encoder {
    fn bit(&mut self, p: &mut Bit, bit: u32) -> u32 {
        let bound = (self.range >> 16) * p.p();
        if bit == 0 {
            self.range = bound;
        } else {
            self.low += bound as u64;
            self.range -= bound;
        }
        p.update(bit);
        while self.range < 1 << 24 {
            self.range <<= 8;
            self.shift_low();
        }
        bit
    }
}

pub struct Decoder<'a> {
    code: u32,
    range: u32,
    input: &'a [u8],
}

impl<'a> Decoder<'a> {
    pub fn new(input: &'a [u8]) -> Self {
        let mut decoder = Self {
            code: 0,
            range: u32::MAX,
            input,
        };
        for _ in 0..5 {
            decoder.code = decoder.code << 8 | decoder.next() as u32;
        }
        decoder
    }

    fn next(&mut self) -> u8 {
        let (&byte, rest) = self.input.split_first().unwrap_or((&0, &[]));
        self.input = rest;
        byte
    }
}

impl Coder for Decoder<'_> {
    fn bit(&mut self, p: &mut Bit, _: u32) -> u32 {
        let bound = (self.range >> 16) * p.p();
        let bit = if self.code < bound {
            self.range = bound;
            0
        } else {
            self.code -= bound;
            self.range -= bound;
            1
        };
        p.update(bit);
        while self.range < 1 << 24 {
            self.range <<= 8;
            self.code = self.code << 8 | self.next() as u32;
        }
        bit
    }
}

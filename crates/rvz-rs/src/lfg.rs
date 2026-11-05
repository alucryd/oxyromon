//! The lagged Fibonacci generator that fills the padding of GameCube and Wii
//! discs, and the reverse: finding the seed that produced some padding, so
//! RVZ can store 68 bytes of seed in its place.
//!
//! A port of Dolphin's `LaggedFibonacciGenerator`, which is CC0. The buffer
//! holds words as Dolphin's does on a little endian host: each word's
//! little endian bytes are the bytes in memory, which is what the generator
//! outputs.

/// Words of seed.
pub const SEED_SIZE: usize = 17;
/// Bytes of seed, as RVZ stores it.
pub const SEED_BYTES: usize = SEED_SIZE * 4;

const K: usize = 521;
const J: usize = 32;

/// A seed, as the words the generator starts from.
pub type Seed = [u32; SEED_SIZE];

pub struct Lfg {
    buffer: [u32; K],
    position: usize,
}

impl Default for Lfg {
    fn default() -> Self {
        Self {
            buffer: [0; K],
            position: 0,
        }
    }
}

impl Lfg {
    /// A generator at the start of the stream `seed` produces.
    pub fn new(seed: &Seed) -> Self {
        let mut lfg = Self::default();
        lfg.buffer[..SEED_SIZE].copy_from_slice(seed);
        lfg.initialize(false);
        lfg
    }

    /// A generator for a seed stored big endian, as RVZ stores it.
    pub fn from_bytes(seed: &[u8]) -> Self {
        let mut words = [0; SEED_SIZE];
        for (word, bytes) in words.iter_mut().zip(seed.as_chunks::<4>().0) {
            *word = u32::from_be_bytes(*bytes);
        }
        Self::new(&words)
    }

    /// Skip `count` bytes of output.
    pub fn forward_bytes(&mut self, count: usize) {
        self.position += count;
        while self.position >= K * 4 {
            self.forward();
            self.position -= K * 4;
        }
    }

    /// Fill `out` with the next bytes of output.
    pub fn fill(&mut self, mut out: &mut [u8]) {
        while !out.is_empty() {
            let length = out.len().min(K * 4 - self.position);
            for (index, byte) in out[..length].iter_mut().enumerate() {
                *byte = self.byte_at(self.position + index);
            }
            self.position += length;
            out = &mut out[length..];
            if self.position == K * 4 {
                self.forward();
                self.position = 0;
            }
        }
    }

    fn byte_at(&self, position: usize) -> u8 {
        self.buffer[position / 4].to_le_bytes()[position % 4]
    }

    fn next_byte(&mut self) -> u8 {
        let byte = self.byte_at(self.position);
        self.position += 1;
        if self.position == K * 4 {
            self.forward();
            self.position = 0;
        }
        byte
    }

    fn forward(&mut self) {
        for i in 0..J {
            self.buffer[i] ^= self.buffer[i + K - J];
        }
        for i in J..K {
            self.buffer[i] ^= self.buffer[i - J];
        }
    }

    fn backward(&mut self, start: usize, end: usize) {
        let loop_end = J.max(start);
        for i in (loop_end + 1..=end.min(K)).rev() {
            self.buffer[i - 1] ^= self.buffer[i - 1 - J];
        }
        for i in (start + 1..=end.min(J)).rev() {
            self.buffer[i - 1] ^= self.buffer[i - 1 + K - J];
        }
    }

    fn initialize(&mut self, check_existing_data: bool) -> bool {
        for i in SEED_SIZE..K {
            let calculated =
                (self.buffer[i - 17] << 23) ^ (self.buffer[i - 16] >> 9) ^ self.buffer[i - 1];
            if check_existing_data {
                let actual = (self.buffer[i] & 0xFF00FFFF) | (self.buffer[i] << 2 & 0x00FC0000);
                if calculated & 0xFFFCFFFF != actual {
                    return false;
                }
            }
            self.buffer[i] = calculated;
        }
        // The output shifts by 18 rather than 16, which is done here, once,
        // along with the byte order of the output
        for word in &mut self.buffer {
            *word = ((*word & 0xFF00FFFF) | ((*word >> 2) & 0x00FF0000)).swap_bytes();
        }
        for _ in 0..4 {
            self.forward();
        }
        true
    }

    fn reinitialize(&mut self, seed: &mut Seed) -> bool {
        for _ in 0..4 {
            self.backward(0, K);
        }
        for word in &mut self.buffer {
            *word = word.swap_bytes();
        }
        // Bits 16 and 17 are lost to the shift by 18; they can be rebuilt for
        // every word but the first, where they make no difference to the output
        for i in 0..SEED_SIZE {
            self.buffer[i] = (self.buffer[i] & 0xFF00FFFF)
                | (self.buffer[i] << 2 & 0x00FC0000)
                | ((self.buffer[i + 16] ^ self.buffer[i + 15]) << 9 & 0x00030000);
        }
        seed.copy_from_slice(&self.buffer[..SEED_SIZE]);
        self.initialize(true)
    }

    fn seed_from_words(words: &[u32], word_offset: usize, lfg: &mut Lfg, seed: &mut Seed) -> bool {
        if words.len() < K {
            return false;
        }
        // Bail out early on what the generator cannot have produced
        if !words[..K]
            .iter()
            .all(|word| word.swap_bytes() & 0x00C00000 == (word.swap_bytes() >> 2) & 0x00C00000)
        {
            return false;
        }
        let offset_mod_k = word_offset % K;
        let offset_div_k = word_offset / K;
        lfg.buffer[offset_mod_k..].copy_from_slice(&words[..K - offset_mod_k]);
        lfg.buffer[..offset_mod_k].copy_from_slice(&words[K - offset_mod_k..K]);
        lfg.backward(0, offset_mod_k);
        for _ in 0..offset_div_k {
            lfg.backward(0, K);
        }
        if !lfg.reinitialize(seed) {
            return false;
        }
        for _ in 0..offset_div_k {
            lfg.forward();
        }
        true
    }

    /// Find the seed whose output, at `data_offset` into a 32 KiB block,
    /// starts `data`, returning how many bytes of `data` it reproduces, 0 if
    /// none.
    pub fn seed(data: &[u8], data_offset: usize, seed: &mut Seed) -> usize {
        // Only whole words are used to find the seed
        let skip = data_offset.next_multiple_of(4) - data_offset;
        if skip > data.len() {
            return 0;
        }
        let words: Vec<u32> = data[skip..]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| u32::from_le_bytes(*bytes))
            .collect();
        let word_offset = (data_offset + skip) / 4;

        let mut lfg = Lfg::default();
        if !Self::seed_from_words(&words, word_offset, &mut lfg, seed) {
            return 0;
        }
        lfg.position = data_offset % (K * 4);
        data.iter()
            .take_while(|&&byte| lfg.next_byte() == byte)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn junk(seed: &Seed, offset: usize, size: usize) -> Vec<u8> {
        let mut lfg = Lfg::new(seed);
        lfg.forward_bytes(offset);
        let mut out = vec![0; size];
        lfg.fill(&mut out);
        out
    }

    #[test]
    fn finds_the_seed_back() {
        let seed: Seed = std::array::from_fn(|i| 0x1234_5678u32.wrapping_mul(i as u32 + 1));
        let data = junk(&seed, 0, 0x8000);
        let mut found = [0; SEED_SIZE];
        assert_eq!(Lfg::seed(&data, 0, &mut found), data.len());
        assert_eq!(junk(&found, 0, 0x8000), data);
    }

    #[test]
    fn finds_the_seed_midway_through_a_block() {
        let seed: Seed = std::array::from_fn(|i| 0x9E37_79B9u32.rotate_left(i as u32));
        let data = junk(&seed, 0x1235, 0x8000 - 0x1235);
        let mut found = [0; SEED_SIZE];
        assert_eq!(Lfg::seed(&data, 0x1235, &mut found), data.len());
        assert_eq!(junk(&found, 0x1235, data.len()), data);
    }

    #[test]
    fn finds_nothing_in_other_data() {
        let data: Vec<u8> = (0..0x8000u32).map(|i| (i * 7 + 3) as u8).collect();
        assert_eq!(Lfg::seed(&data, 0, &mut [0; SEED_SIZE]), 0);
    }
}

//! Bit-level reader and writer over byte buffers.
//!
//! A port of MAME's `bitstream_in` and `bitstream_out`
//! (`src/lib/util/bitstream.h`): bits are packed most significant bit
//! first, and reading past the end of the buffer yields zero bits.

/// A reader that consumes bits from a byte slice, most significant bit
/// first.
pub(crate) struct BitstreamIn<'a> {
    data: &'a [u8],
    pos: u64,
    overflow: bool,
}

impl<'a> BitstreamIn<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            overflow: false,
        }
    }

    /// Whether bits were consumed past the end of the buffer.
    pub fn overflow(&self) -> bool {
        self.overflow
    }

    /// Fetches the requested number of bits without advancing the input
    /// pointer. Bits past the end of the buffer read as zero.
    pub fn peek(&self, numbits: u32) -> u32 {
        if numbits == 0 {
            return 0;
        }
        // the eight bytes from the current one, zeros past the end, hold
        // any 32 bits from any bit offset
        let start = (self.pos / 8) as usize;
        let mut window = [0u8; 8];
        if let Some(available) = self.data.get(start..) {
            let count = available.len().min(8);
            window[..count].copy_from_slice(&available[..count]);
        }
        let bits = u64::from_be_bytes(window) << (self.pos % 8);
        (bits >> (64 - numbits)) as u32
    }

    fn advance(&mut self, numbits: u32) {
        self.pos += u64::from(numbits);
        if self.pos > self.data.len() as u64 * 8 {
            self.overflow = true;
        }
    }

    /// Advances the input pointer by the specified number of bits.
    pub fn remove(&mut self, numbits: u32) {
        self.advance(numbits);
    }

    /// Fetches the requested number of bits.
    pub fn read(&mut self, numbits: u32) -> u32 {
        let result = self.peek(numbits);
        self.advance(numbits);
        result
    }

    /// Skips to the next byte boundary, returning the number of bytes
    /// consumed so far.
    pub fn flush(&mut self) -> usize {
        self.pos = self.pos.div_ceil(8) * 8;
        (self.pos / 8) as usize
    }
}

/// A writer that packs bits into a byte buffer, most significant bit
/// first.
pub(crate) struct BitstreamOut<'a> {
    buffer: u32,
    bits: u32,
    data: &'a mut [u8],
    doffset: usize,
}

impl<'a> BitstreamOut<'a> {
    pub fn new(data: &'a mut [u8]) -> Self {
        Self {
            buffer: 0,
            bits: 0,
            data,
            doffset: 0,
        }
    }

    /// Writes the given number of bits to the data stream.
    pub fn write(&mut self, mut newbits: u32, mut numbits: u32) {
        if numbits == 0 {
            return;
        }
        debug_assert!(numbits <= 32);

        // Shift the bits up to fill the accumulator from the top.
        newbits <<= 32 - numbits;

        // Flush the accumulator while it would overflow.
        while self.bits + numbits >= 32 && numbits > 0 {
            while self.bits >= 8 {
                if self.doffset < self.data.len() {
                    self.data[self.doffset] = (self.buffer >> 24) as u8;
                }
                self.doffset += 1;
                self.buffer <<= 8;
                self.bits -= 8;
            }
            // Offload as many bits as the accumulator can still hold.
            if self.bits + numbits >= 32 {
                let rem = (32 - self.bits).min(numbits);
                self.buffer |= newbits >> self.bits;
                self.bits += rem;
                newbits = if rem == 32 { 0 } else { newbits << rem };
                numbits -= rem;
            }
        }

        // Shift down to account for the number of bits the accumulator
        // already holds, and OR them in.
        if numbits > 0 {
            self.buffer |= newbits >> self.bits;
            self.bits += numbits;
        }
    }

    /// Whether writes have run past the end of the buffer.
    pub fn overflow(&self) -> bool {
        self.doffset > self.data.len()
    }

    /// Outputs the remaining bits and returns the final output size in
    /// bytes.
    pub fn flush(&mut self) -> usize {
        while self.bits > 0 {
            if self.doffset < self.data.len() {
                self.data[self.doffset] = (self.buffer >> 24) as u8;
            }
            self.doffset += 1;
            self.buffer <<= 8;
            self.bits = self.bits.saturating_sub(8);
        }
        self.bits = 0;
        self.buffer = 0;
        self.doffset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_packs_msb_first() {
        let data = [0b1011_0100, 0b0010_0000];
        let mut stream = BitstreamIn::new(&data);
        assert_eq!(stream.read(1), 1);
        assert_eq!(stream.read(3), 0b011);
        assert_eq!(stream.read(4), 0b0100);
        assert_eq!(stream.read(5), 0b00100);
        assert!(!stream.overflow());
    }

    #[test]
    fn read_past_end_yields_zero() {
        let data = [0xff];
        let mut stream = BitstreamIn::new(&data);
        assert_eq!(stream.read(12), 0xff0);
        assert!(stream.overflow());
    }

    #[test]
    fn peek_does_not_advance() {
        let data = [0b1010_0000];
        let mut stream = BitstreamIn::new(&data);
        assert_eq!(stream.peek(2), 0b10);
        assert_eq!(stream.peek(2), 0b10);
        assert_eq!(stream.read(2), 0b10);
        assert_eq!(stream.read(1), 0b1);
    }

    #[test]
    fn peek_past_end_sets_no_overflow_until_consumed() {
        let data = [0b1110_0000];
        let mut stream = BitstreamIn::new(&data);
        assert_eq!(stream.peek(8), 0b1110_0000);
        assert!(!stream.overflow());
        stream.remove(3);
        assert!(!stream.overflow());
        stream.remove(6);
        assert!(stream.overflow());
    }

    #[test]
    fn write_round_trips_through_read() {
        let mut data = [0u8; 4];
        {
            let mut out = BitstreamOut::new(&mut data);
            out.write(0b1, 1);
            out.write(0b011, 3);
            out.write(0b0100, 4);
            out.write(0b00100, 5);
            out.write(0b11111, 5);
            assert_eq!(out.flush(), 3);
        }
        assert_eq!(data[..3], [0b1011_0100, 0b0010_0111, 0b1100_0000]);

        let mut stream = BitstreamIn::new(&data);
        assert_eq!(stream.read(1), 0b1);
        assert_eq!(stream.read(3), 0b011);
        assert_eq!(stream.read(4), 0b0100);
        assert_eq!(stream.read(5), 0b00100);
        assert_eq!(stream.read(5), 0b11111);
    }

    #[test]
    fn write_matches_mame_bitstream_out() {
        // Reference values traced through MAME's bitstream_out: writing
        // 32 bits at once, then odd widths across byte boundaries.
        let mut data = [0u8; 8];
        {
            let mut out = BitstreamOut::new(&mut data);
            out.write(0xdead_beef, 32);
            out.write(0b101, 3);
            out.write(0x1_ffff, 17);
            out.write(0b01, 2);
            out.flush();
        }
        assert_eq!(data, [0xde, 0xad, 0xbe, 0xef, 0xbf, 0xff, 0xf4, 0x00]);
    }

    #[test]
    fn write_offloads_large_codes() {
        // A write that the accumulator cannot hold in one go.
        let mut data = [0u8; 5];
        {
            let mut out = BitstreamOut::new(&mut data);
            out.write(0b101, 3);
            out.write(0x0fff_ffff, 28);
            out.write(0b11, 2);
            out.flush();
        }
        let mut stream = BitstreamIn::new(&data);
        assert_eq!(stream.read(3), 0b101);
        assert_eq!(stream.read(28), 0x0fff_ffff);
        assert_eq!(stream.read(2), 0b11);
    }
}

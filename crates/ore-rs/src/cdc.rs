// SPDX-License-Identifier: GPL-3.0-or-later
//! MinCDC, as MinCdcHash4 scores it: a chunk ends after the 4-byte window
//! whose hash is lowest between [`MIN`] and [`MAX`] bytes in, the earliest
//! on a tie. Ported rather than depended on, so that a dependency's update
//! can never move a boundary: the parameters are the format's.

/// Chunk sizes, 16 KiB on average.
pub const MIN: usize = 8 << 10;
pub const MAX: usize = 24 << 10;
const MULTIPLIER: u32 = 0x915F_77F5;
const ADDEND: u32 = 0x3463_6463;

/// The length of the chunk `data` starts with. `data` must hold more than
/// [`MAX`] bytes, or be all there is.
pub fn cut(data: &[u8]) -> usize {
    if data.len() <= MIN {
        return data.len();
    }
    let search = &data[MIN - 4..MAX.min(data.len())];
    let mut best = 0;
    let mut lowest = u32::MAX;
    for (i, window) in search.windows(4).enumerate() {
        let score = u32::from_le_bytes(window.try_into().unwrap())
            .wrapping_mul(MULTIPLIER)
            .wrapping_add(ADDEND);
        if score < lowest {
            lowest = score;
            best = i;
        }
    }
    MIN + best
}

#[cfg(test)]
mod tests {
    use super::*;
    use mothcdc::mincdc::{MinCdcHash4, SliceChunker};

    #[test]
    fn cuts_as_mothcdc_does() {
        // Text-like data, so boundaries depend on content
        let mut state = 0x1234_5678_u64;
        let data: Vec<u8> = (0..3 << 20)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                b'a' + (state % 7) as u8
            })
            .collect();
        let expected: Vec<usize> = SliceChunker::new(&data, MIN, MAX, MinCdcHash4::new())
            .map(|chunk| chunk.len())
            .collect();
        let mut ours = Vec::new();
        let mut at = 0;
        while at < data.len() {
            let length = cut(&data[at..]);
            ours.push(length);
            at += length;
        }
        assert_eq!(ours, expected);
    }
}

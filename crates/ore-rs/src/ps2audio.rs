// SPDX-License-Identifier: GPL-3.0-or-later
//! Audio on a PlayStation 2 DVD, found by its headers: Sony's `SShd` and
//! `VAGp`, and `RIFF WAVE`. What is found becomes an area of its own,
//! coded by `vag` or `pcm` rather than with the rest. A header that lies
//! only costs size: both codecs give back any bytes.

use super::format::Codec;

/// The bytes past a window a header is read whole from.
pub const LOOKAHEAD: usize = 4096;
/// Streams of one kind this close are one area, the bytes between them
/// with them.
const GAP: u64 = 64 << 10;
/// Less audio than this stays with the rest.
const AREA_MIN: u64 = 64 << 10;
/// The `VAGp` header, before its blocks.
const VAG_HEADER: u64 = 0x30;

/// A stretch of the disc that is audio, and its codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Audio {
    pub start: u64,
    pub len: u64,
    pub codec: Codec,
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(b[at..at + 4].try_into().unwrap())
}

/// The audio whose header starts `b`, at `at` on a disc of `size` bytes,
/// if its header is plausible: a VAG with its header, a PCM stream's
/// samples alone, to a whole period of its channels.
fn stream(b: &[u8], at: u64, size: u64) -> Option<Audio> {
    let rate_ok = |rate: u32| (4000..=96000).contains(&rate);
    let fits = |start: u64, len: u64| len > 0 && start + len <= size;
    let pcm = |start: u64, len: u64, period: u64| Audio {
        start,
        len: len / period * period,
        codec: Codec::Pcm,
    };
    match &b[..4] {
        // Sony's ADS: SShd, then SSbd and its size; format 1 is PCM
        b"SShd" if le32(b, 4) == 0x18 && &b[0x20..0x24] == b"SSbd" => {
            let (format, rate, channels, interleave) =
                (le32(b, 8), le32(b, 12), le32(b, 16), le32(b, 20));
            let period = match (channels, interleave) {
                (1, _) => 2,
                (2, 512) => 1024,
                _ => return None,
            };
            let (start, len) = (at + 0x28, u64::from(le32(b, 0x24)));
            (format == 1 && rate_ok(rate) && fits(start, len)).then(|| pcm(start, len, period))
        }
        b"VAGp" => {
            let (len, rate) = (u64::from(be32(b, 0x0c)), be32(b, 0x10));
            (rate_ok(rate) && len % 16 == 0 && fits(at + VAG_HEADER, len)).then_some(Audio {
                start: at,
                len: VAG_HEADER + len,
                codec: Codec::Vag,
            })
        }
        // A WAVE of 16-bit PCM, its fmt chunk first, then its data
        b"RIFF" if &b[8..16] == b"WAVEfmt " => {
            let fmt = le32(b, 16) as usize;
            let (format, channels, rate, bits) = (
                u16::from_le_bytes([b[20], b[21]]),
                u16::from_le_bytes([b[22], b[23]]),
                le32(b, 24),
                u16::from_le_bytes([b[34], b[35]]),
            );
            let data = 20 + fmt;
            if format != 1
                || bits != 16
                || !(1..=2).contains(&channels)
                || data + 8 > b.len()
                || &b[data..data + 4] != b"data"
            {
                return None;
            }
            let (start, len) = (at + data as u64 + 8, u64::from(le32(b, data + 4)));
            (rate_ok(rate) && fits(start, len)).then(|| pcm(start, len, 2 * channels as u64))
        }
        _ => None,
    }
}

/// Audio looked for at every 16 bytes of a disc fed in order, the scan
/// skipping past each stream found.
pub struct Scanner {
    size: u64,
    next: u64,
    found: Vec<Audio>,
}

impl Scanner {
    pub fn new(size: u64) -> Self {
        Self {
            size,
            next: 0,
            found: Vec::new(),
        }
    }

    /// Look for headers in the first `scan` bytes of `window`, which lies at
    /// `at` and holds up to [`LOOKAHEAD`] bytes more.
    pub fn feed(&mut self, window: &[u8], at: u64, scan: usize) {
        let mut i = self.next.saturating_sub(at).next_multiple_of(16) as usize;
        while i < scan && i + 64 <= window.len() {
            match stream(&window[i..], at + i as u64, self.size) {
                Some(audio) if audio.len > 0 => {
                    self.next = (audio.start + audio.len).next_multiple_of(16);
                    self.found.push(audio);
                    i = self.next.saturating_sub(at) as usize;
                }
                _ => i += 16,
            }
        }
    }

    /// The audio found, streams of a kind close together as one, sorted.
    pub fn finish(self) -> Vec<Audio> {
        let mut areas: Vec<Audio> = Vec::new();
        for audio in self.found {
            match areas.last_mut() {
                Some(last)
                    if last.codec == audio.codec
                        && audio.start >= last.start + last.len
                        && audio.start - (last.start + last.len) <= GAP
                        // A VAG's blocks stay on 16 bytes from the area's start
                        && (audio.start - last.start) % 16 == 0 =>
                {
                    last.len = audio.start + audio.len - last.start;
                }
                Some(last) if audio.start < last.start + last.len => {}
                _ => areas.push(audio),
            }
        }
        areas.retain(|area| area.len >= AREA_MIN);
        areas
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_close_together_are_one_area() {
        let mut disc = vec![0u8; 1 << 20];
        let vag = |disc: &mut [u8], at: usize, len: u32| {
            disc[at..at + 4].copy_from_slice(b"VAGp");
            disc[at + 0x0c..at + 0x10].copy_from_slice(&len.to_be_bytes());
            disc[at + 0x10..at + 0x14].copy_from_slice(&22_050u32.to_be_bytes());
        };
        vag(&mut disc, 0x1000, 0x10000);
        vag(&mut disc, 0x12000, 0x8000);
        // Too far from them, and too short alone
        vag(&mut disc, 0x80000, 0x100);
        let mut scanner = Scanner::new(disc.len() as u64);
        scanner.feed(&disc, 0, disc.len());
        assert_eq!(
            scanner.finish(),
            vec![Audio {
                start: 0x1000,
                len: 0x12000 + 0x30 + 0x8000 - 0x1000,
                codec: Codec::Vag
            }]
        );
    }
}

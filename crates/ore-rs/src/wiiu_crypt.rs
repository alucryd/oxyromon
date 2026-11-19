// SPDX-License-Identifier: MIT OR Apache-2.0
//! A Wii U region's encryption put back: its bytes are stored decrypted,
//! and read back encrypted as the disc has them. A region is encrypted by
//! the disc key whole, the outer layer, after its titles' contents within
//! it are by their title keys, the pieces. Each layer is AES-128-CBC:
//!
//! - a chain, from the IV of its index, the cluster's or the content's, in
//!   its first two bytes, its IV every 64 KiB kept so a read starts there;
//! - 64 KiB blocks, each a chain of its own, the first from its index's IV,
//!   the others from their block number in the last eight bytes;
//! - hashed 64 KiB blocks: 1 KiB of hashes, a chain from a zero IV, then
//!   the data, a chain from the first 16 bytes of its block's hash in them,
//!   its hash the block's position among each 16.

use crate::crypto::{Key, cbc_decrypt, cbc_encrypt};
use crate::error::{Error, Result};
use crate::format::{Layer, LayerMode, Wiiu};

/// The block every layer counts in, and a chain keeps its IV at.
pub const BLOCK: u64 = 0x10000;
const HASHES: usize = 0x400;

/// The IV of block `block` of index `index`.
pub fn block_iv(index: u16, block: u64) -> [u8; 16] {
    let mut iv = [0; 16];
    match block {
        0 => iv[..2].copy_from_slice(&index.to_be_bytes()),
        _ => iv[8..].copy_from_slice(&block.to_be_bytes()),
    }
    iv
}

/// A layer, checked: its key, mode, index and kept IVs.
#[derive(Debug, Clone)]
pub struct Crypt {
    pub mode: LayerMode,
    pub key: Key,
    pub index: u16,
    pub ivs: Vec<[u8; 16]>,
}

/// A region's layers, checked against its length: pieces in order, apart,
/// within it, each a whole number of its blocks or AES blocks.
#[derive(Debug, Clone)]
pub struct Layers {
    pub outer: Option<Crypt>,
    pub pieces: Vec<(u64, u64, Crypt)>,
}

fn corrupt(what: &str) -> Error {
    Error::Corrupt(format!("a Wii U region's {what}"))
}

impl Crypt {
    fn new(layer: &Layer, len: u64) -> Result<Crypt> {
        let key = Key::try_from(layer.key.as_slice()).map_err(|_| corrupt("key"))?;
        let chains = match layer.mode {
            LayerMode::Chain => len.div_ceil(BLOCK).saturating_sub(1),
            LayerMode::Blocks | LayerMode::Hashed => 0,
        };
        if layer.ivs.len() as u64 != chains * 16
            || !len.is_multiple_of(16)
            || (layer.mode == LayerMode::Hashed && !len.is_multiple_of(BLOCK))
        {
            return Err(corrupt("layer"));
        }
        Ok(Crypt {
            mode: layer.mode,
            key,
            index: layer.index,
            ivs: layer.ivs.as_chunks::<16>().0.to_vec(),
        })
    }

    /// The IV a chain starts with `at` bytes into it, at a 64 KiB boundary.
    fn iv(&self, at: u64) -> [u8; 16] {
        match at / BLOCK {
            0 => block_iv(self.index, 0),
            n => self.ivs[n as usize - 1],
        }
    }

    /// Decrypt `buf`, `at` bytes into the layer, as [`Crypt::encrypt`]
    /// encrypts it.
    fn decrypt(&self, at: u64, buf: &mut [u8]) {
        match self.mode {
            LayerMode::Chain => cbc_decrypt(&self.key, &self.iv(at), buf),
            LayerMode::Blocks => {
                for (i, block) in buf.chunks_mut(BLOCK as usize).enumerate() {
                    cbc_decrypt(
                        &self.key,
                        &block_iv(self.index, at / BLOCK + i as u64),
                        block,
                    );
                }
            }
            LayerMode::Hashed => {
                for (i, block) in buf.chunks_mut(BLOCK as usize).enumerate() {
                    let slot = ((at / BLOCK + i as u64) % 16) as usize * 20;
                    let (hashes, data) = block.split_at_mut(HASHES);
                    cbc_decrypt(&self.key, &[0; 16], hashes);
                    let iv: [u8; 16] = hashes[slot..slot + 16].try_into().unwrap();
                    cbc_decrypt(&self.key, &iv, data);
                }
            }
        }
    }

    /// Decrypt `buf`, `at` bytes into an outer layer, anywhere on an AES
    /// block: chained from `before`, the ciphertext before it, but where a
    /// chain starts.
    #[cfg_attr(not(feature = "write"), allow(dead_code))]
    fn decrypt_from(&self, at: u64, before: &[u8; 16], buf: &mut [u8]) {
        let iv = |at: u64| match (self.mode, at % BLOCK, at) {
            (LayerMode::Blocks, 0, _) => block_iv(self.index, at / BLOCK),
            (_, _, 0) => block_iv(self.index, 0),
            _ => *before,
        };
        match self.mode {
            // Each block from its own IV: only the first may start within one
            LayerMode::Blocks => {
                let first = ((BLOCK - at % BLOCK) % BLOCK) as usize;
                let (head, tail) = buf.split_at_mut(first.min(buf.len()));
                if !head.is_empty() {
                    cbc_decrypt(&self.key, &iv(at), head);
                }
                let at = at + head.len() as u64;
                for (i, block) in tail.chunks_mut(BLOCK as usize).enumerate() {
                    cbc_decrypt(&self.key, &iv(at + i as u64 * BLOCK), block);
                }
            }
            _ => cbc_decrypt(&self.key, &iv(at), buf),
        }
    }

    /// Encrypt `buf`, `at` bytes into the layer: a 64 KiB boundary, and the
    /// layer's end or one after `buf`.
    fn encrypt(&self, at: u64, buf: &mut [u8]) {
        match self.mode {
            LayerMode::Chain => cbc_encrypt(&self.key, &self.iv(at), buf),
            LayerMode::Blocks => {
                for (i, block) in buf.chunks_mut(BLOCK as usize).enumerate() {
                    cbc_encrypt(
                        &self.key,
                        &block_iv(self.index, at / BLOCK + i as u64),
                        block,
                    );
                }
            }
            LayerMode::Hashed => {
                for (i, block) in buf.chunks_mut(BLOCK as usize).enumerate() {
                    let slot = ((at / BLOCK + i as u64) % 16) as usize * 20;
                    let (hashes, data) = block.split_at_mut(HASHES);
                    let iv: [u8; 16] = hashes[slot..slot + 16].try_into().unwrap();
                    cbc_encrypt(&self.key, &[0; 16], hashes);
                    cbc_encrypt(&self.key, &iv, data);
                }
            }
        }
    }
}

impl Layers {
    /// `wiiu`'s layers over a region of `len` bytes.
    pub fn new(wiiu: &Wiiu, len: u64) -> Result<Layers> {
        if !len.is_multiple_of(16) {
            return Err(corrupt("length"));
        }
        let outer = wiiu
            .outer
            .as_ref()
            .map(|layer| Crypt::new(layer, len))
            .transpose()?;
        let mut pieces = Vec::with_capacity(wiiu.pieces.len());
        let mut end = 0;
        for piece in &wiiu.pieces {
            if piece.offset < end
                || !piece.offset.is_multiple_of(16)
                || piece.len == 0
                || piece.offset.checked_add(piece.len).is_none_or(|e| e > len)
                || piece.layer.mode == LayerMode::Blocks
            {
                return Err(corrupt("pieces"));
            }
            end = piece.offset + piece.len;
            pieces.push((
                piece.offset,
                piece.len,
                Crypt::new(&piece.layer, piece.len)?,
            ));
        }
        Ok(Layers { outer, pieces })
    }

    /// What a read of `from..to` of a region of `len` bytes needs: the
    /// outer layer's 64 KiB blocks it touches, which come out encrypted,
    /// and around them, the plaintext of the inner blocks those touch,
    /// each one at most, which comes out encrypted by its piece alone.
    pub fn span(&self, len: u64, from: u64, to: u64) -> Span {
        let (from, to) = match self.outer {
            Some(_) => (from / BLOCK * BLOCK, to.next_multiple_of(BLOCK).min(len)),
            None => (from / 16 * 16, to.next_multiple_of(16).min(len)),
        };
        let (mut lo, mut hi) = (from, to);
        for (offset, piece, crypt) in &self.pieces {
            let end = offset + piece;
            if lo > *offset && lo < end {
                lo = offset + (lo - offset) / BLOCK * BLOCK;
            }
            // A chain encrypts forward from where it starts; a hashed
            // block, whole
            if hi > *offset && hi < end && crypt.mode == LayerMode::Hashed {
                hi = (offset + (hi - offset).next_multiple_of(BLOCK)).min(end);
            }
        }
        Span { lo, hi, from, to }
    }

    /// Encrypt `buf`, the region's plaintext from `span.lo` to `span.hi`:
    /// from `span.from` to `span.to`, it is then as the disc has it.
    pub fn encrypt(&self, span: &Span, buf: &mut [u8]) {
        self.pieces(span, buf, Crypt::encrypt);
        if let Some(outer) = &self.outer {
            let range = (span.from - span.lo) as usize..(span.to - span.lo) as usize;
            outer.encrypt(span.from, &mut buf[range]);
        }
    }

    /// Decrypt `buf`, the region's bytes as the disc has them from
    /// `span.lo` to `span.hi`, `before` the 16 before them: from
    /// `span.from` to `span.to`, it is then what [`Layers::encrypt`]
    /// encrypts back.
    #[cfg_attr(not(feature = "write"), allow(dead_code))]
    pub fn decrypt(&self, span: &Span, before: &[u8; 16], buf: &mut [u8]) {
        if let Some(outer) = &self.outer {
            outer.decrypt_from(span.lo, before, buf);
        }
        self.pieces(span, buf, Crypt::decrypt);
    }

    /// Apply `crypt` to each piece's bytes in `buf`, from `span.lo`.
    fn pieces(&self, span: &Span, buf: &mut [u8], crypt: fn(&Crypt, u64, &mut [u8])) {
        for (offset, piece, layer) in &self.pieces {
            let (start, end) = (span.lo.max(*offset), span.hi.min(offset + piece));
            if start < end {
                crypt(
                    layer,
                    start - offset,
                    &mut buf[(start - span.lo) as usize..(end - span.lo) as usize],
                );
            }
        }
    }
}

/// A read's extent in a region: the plaintext it needs, `lo` to `hi`, and
/// what of it comes out as the disc has it, `from` to `to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub lo: u64,
    pub hi: u64,
    pub from: u64,
    pub to: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(len: usize, mut state: u64) -> Vec<u8> {
        (0..len)
            .map(|_| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                (state >> 56) as u8
            })
            .collect()
    }

    fn crypt(mode: LayerMode, key: u8, index: u16) -> Crypt {
        Crypt {
            mode,
            key: [key; 16],
            index,
            ivs: Vec::new(),
        }
    }

    /// A chain's IVs, from what it encrypted: the 16 bytes before each
    /// 64 KiB of it.
    fn ivs(encrypted: &[u8]) -> Vec<[u8; 16]> {
        (1..(encrypted.len() as u64).div_ceil(BLOCK))
            .map(|k| {
                let at = (k * BLOCK) as usize;
                encrypted[at - 16..at].try_into().unwrap()
            })
            .collect()
    }

    /// Pieces whose blocks start off the outer layer's, as an update
    /// partition's titles do: a read needs a block of each around it, and
    /// no more, and decrypts and encrypts back to the disc's bytes.
    #[test]
    fn a_read_takes_the_blocks_around_it_alone() {
        let len = 20 * BLOCK;
        for outer_mode in [LayerMode::Chain, LayerMode::Blocks] {
            let plain = noise(len as usize, 1);
            let mut layers = Layers {
                outer: None,
                pieces: vec![
                    (0x30, 2 * BLOCK + 0x40, crypt(LayerMode::Chain, 1, 3)),
                    (3 * BLOCK + 0x20, 6 * BLOCK, crypt(LayerMode::Hashed, 2, 4)),
                    (12 * BLOCK + 0x7F0, 5 * BLOCK, crypt(LayerMode::Chain, 3, 5)),
                ],
            };
            // Encrypted whole, from each chain's start, its IVs from that
            let whole = |layers: &Layers, buf: &mut [u8]| {
                let span = Span {
                    lo: 0,
                    hi: len,
                    from: 0,
                    to: len,
                };
                layers.encrypt(&span, buf);
            };
            let mut inner = plain.clone();
            whole(&layers, &mut inner);
            for (offset, piece, layer) in &mut layers.pieces {
                if layer.mode == LayerMode::Chain {
                    layer.ivs = ivs(&inner[*offset as usize..(*offset + *piece) as usize]);
                }
            }
            let mut disc = inner.clone();
            let outer = crypt(outer_mode, 9, 7);
            outer.encrypt(0, &mut disc);
            layers.outer = Some(Crypt {
                ivs: match outer_mode {
                    LayerMode::Chain => ivs(&disc),
                    _ => Vec::new(),
                },
                ..outer
            });

            let mut state = 5;
            for _ in 0..200 {
                let from = noise(8, state).iter().fold(0u64, |a, &b| a << 8 | b as u64) % len;
                state += 1;
                let to = (from
                    + noise(4, state)
                        .iter()
                        .fold(1u64, |a, &b| a + b as u64 * 300))
                .min(len);
                state += 1;
                let span = layers.span(len, from, to);
                assert!(span.from <= from && span.to >= to);
                assert!(
                    span.hi - span.lo <= (to - from) + 4 * BLOCK,
                    "{outer_mode:?} {from:#x}..{to:#x}: {span:?}"
                );
                let range = span.lo as usize..span.hi as usize;
                let window = (span.from - span.lo) as usize..(span.to - span.lo) as usize;
                let at = (span.from as usize)..(span.to as usize);
                let mut before = [0; 16];
                if span.lo > 0 {
                    before.copy_from_slice(&disc[span.lo as usize - 16..span.lo as usize]);
                }
                let mut decrypted = disc[range.clone()].to_vec();
                layers.decrypt(&span, &before, &mut decrypted);
                assert!(decrypted[window.clone()] == plain[at.clone()], "{span:?}");
                let mut encrypted = plain[range].to_vec();
                layers.encrypt(&span, &mut encrypted);
                assert!(encrypted[window] == disc[at], "{span:?}");
            }
        }
    }
}

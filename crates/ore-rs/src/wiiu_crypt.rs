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

    /// What a read of `from..to` must encrypt, of a region of `len` bytes:
    /// the 64 KiB blocks of every layer it touches, whole.
    pub fn span(&self, len: u64, from: u64, to: u64) -> (u64, u64) {
        let (mut lo, mut hi) = (from, to);
        loop {
            let before = (lo, hi);
            if self.outer.is_some() {
                lo = lo / BLOCK * BLOCK;
                hi = hi.next_multiple_of(BLOCK).min(len);
            }
            for &(offset, piece, _) in &self.pieces {
                let end = offset + piece;
                if lo > offset && lo < end {
                    lo = offset + (lo - offset) / BLOCK * BLOCK;
                }
                if hi > offset && hi < end {
                    hi = (offset + (hi - offset).next_multiple_of(BLOCK)).min(end);
                }
            }
            // The AES block, where no layer is
            lo = lo / 16 * 16;
            hi = hi.next_multiple_of(16).min(len);
            if (lo, hi) == before {
                return (lo, hi);
            }
        }
    }

    /// Encrypt `buf`, the region's decrypted bytes from `lo`, as [`span`]
    /// gives it.
    ///
    /// [`span`]: Layers::span
    pub fn encrypt(&self, lo: u64, buf: &mut [u8]) {
        let hi = lo + buf.len() as u64;
        for (offset, piece, crypt) in &self.pieces {
            let (start, end) = (lo.max(*offset), hi.min(offset + piece));
            if start < end {
                crypt.encrypt(
                    start - offset,
                    &mut buf[(start - lo) as usize..(end - lo) as usize],
                );
            }
        }
        if let Some(outer) = &self.outer {
            outer.encrypt(lo, buf);
        }
    }

    /// Decrypt `buf`, the region's bytes as the disc has them from `lo`, as
    /// [`span`] gives it: what [`Layers::encrypt`] encrypts back.
    ///
    /// [`span`]: Layers::span
    #[cfg_attr(not(feature = "write"), allow(dead_code))]
    pub fn decrypt(&self, lo: u64, buf: &mut [u8]) {
        if let Some(outer) = &self.outer {
            outer.decrypt(lo, buf);
        }
        let hi = lo + buf.len() as u64;
        for (offset, piece, crypt) in &self.pieces {
            let (start, end) = (lo.max(*offset), hi.min(offset + piece));
            if start < end {
                crypt.decrypt(
                    start - offset,
                    &mut buf[(start - lo) as usize..(end - lo) as usize],
                );
            }
        }
    }
}

//! Seekable AES-128-CTR as used by NCA section crypto.
//!
//! Mirrors `nsz.nut.aes128.AESCTR` / the `AESCTR` in
//! `IndependentNczDecompressor.py`: the counter block is the first 8 bytes of the
//! nonce followed by a 64-bit big-endian counter whose value at an absolute byte
//! offset `off` is `off >> 4`. This makes the keystream a pure function of the
//! absolute offset, so any position can be seeked to without replaying.

use aes::cipher::{generic_array::GenericArray, BlockEncrypt, KeyInit};
use aes::Aes128;

use crate::crypto::ecb::BLOCK_SIZE;

/// Counter blocks encrypted per batch (lets AES-NI pipeline several blocks).
const BATCH: usize = 256;

/// XOR `buf` in place with the CTR keystream for absolute offset `abs_offset`.
///
/// `nonce` is 16 bytes; only its first 8 bytes are used as the counter prefix
/// (matching `Counter.new(64, prefix=nonce[0:8], initial_value=off>>4)`).
pub fn keystream_xor(key: &[u8; 16], nonce: &[u8; 16], abs_offset: u64, buf: &mut [u8]) {
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut counter = abs_offset >> 4;
    let mut skip = (abs_offset & 0xF) as usize; // keystream bytes to discard in the first block
    let mut blocks = [GenericArray::<u8, _>::default(); BATCH];
    let mut pos = 0;
    while pos < buf.len() {
        let n = (skip + buf.len() - pos).div_ceil(BLOCK_SIZE).min(BATCH);
        for b in &mut blocks[..n] {
            b[..8].copy_from_slice(&nonce[..8]);
            b[8..].copy_from_slice(&counter.to_be_bytes());
            counter += 1;
        }
        cipher.encrypt_blocks(&mut blocks[..n]);
        let keystream = blocks[..n].iter().flat_map(|b| b.iter()).skip(skip);
        for (byte, k) in buf[pos..].iter_mut().zip(keystream) {
            *byte ^= k;
            pos += 1;
        }
        skip = 0;
    }
}

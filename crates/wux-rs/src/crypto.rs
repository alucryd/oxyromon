//! AES-128-CBC decryption, the only cipher mode Wii U discs use.

use aes::Aes128;
use aes::cipher::{Array, BlockCipherDecrypt, KeyInit};

/// An AES-128 key.
pub type Key = [u8; 16];

/// Blocks decrypted per batch, which lets AES-NI pipeline them.
const BATCH: usize = 256;

/// Decrypt `buf` in place, continuing the chain from `iv`, which is left as
/// the IV of whatever follows `buf`. `buf`'s length must be a multiple of 16.
pub fn cbc_decrypt(key: &Key, iv: &mut [u8; 16], buf: &mut [u8]) {
    debug_assert!(buf.len().is_multiple_of(16));
    let cipher = Aes128::new(key.into());
    let mut blocks = [Array::<u8, _>::default(); BATCH];
    for chunk in buf.chunks_mut(16 * BATCH) {
        let n = chunk.len() / 16;
        for (block, bytes) in blocks.iter_mut().zip(chunk.as_chunks::<16>().0) {
            block.copy_from_slice(bytes);
        }
        cipher.decrypt_blocks(&mut blocks[..n]);
        for (block, bytes) in blocks.iter().zip(chunk.as_chunks_mut::<16>().0) {
            let next_iv = *bytes;
            for ((byte, plain), chain) in bytes.iter_mut().zip(block.iter()).zip(iv.iter()) {
                *byte = plain ^ chain;
            }
            *iv = next_iv;
        }
    }
}

/// Decrypt one 16-byte block chained from `iv`.
pub fn decrypt_block(key: &Key, iv: &[u8; 16], block: &[u8; 16]) -> [u8; 16] {
    let mut out = *block;
    cbc_decrypt(key, &mut iv.clone(), &mut out);
    out
}

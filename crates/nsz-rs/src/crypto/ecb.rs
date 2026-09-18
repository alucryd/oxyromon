//! AES-128 ECB block primitives.
//!
//! Mirrors `nsz.nut.aes128.AESECB` for the block-aligned operations the rest of
//! the library actually uses (key derivation, keyblock unwrap).

use aes::cipher::{generic_array::GenericArray, BlockDecrypt, BlockEncrypt, KeyInit};
use aes::Aes128;

pub const BLOCK_SIZE: usize = 0x10;

/// Encrypt a single 16-byte block in place with an AES-128 ECB key.
pub fn encrypt_block(key: &[u8; 16], block: &mut [u8; 16]) {
    let cipher = Aes128::new(GenericArray::from_slice(key));
    cipher.encrypt_block(GenericArray::from_mut_slice(block));
}

/// Decrypt a single 16-byte block in place with an AES-128 ECB key.
pub fn decrypt_block(key: &[u8; 16], block: &mut [u8; 16]) {
    let cipher = Aes128::new(GenericArray::from_slice(key));
    cipher.decrypt_block(GenericArray::from_mut_slice(block));
}

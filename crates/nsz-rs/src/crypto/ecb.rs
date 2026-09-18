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

/// ECB-encrypt a block-aligned buffer (len must be a multiple of 16).
pub fn encrypt_blocks(key: &[u8; 16], data: &[u8]) -> Vec<u8> {
    assert!(
        data.len().is_multiple_of(BLOCK_SIZE),
        "ecb: data not block aligned"
    );
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut out = data.to_vec();
    for chunk in out.as_chunks_mut::<BLOCK_SIZE>().0 {
        cipher.encrypt_block(GenericArray::from_mut_slice(chunk));
    }
    out
}

/// ECB-decrypt a block-aligned buffer (len must be a multiple of 16).
pub fn decrypt_blocks(key: &[u8; 16], data: &[u8]) -> Vec<u8> {
    assert!(
        data.len().is_multiple_of(BLOCK_SIZE),
        "ecb: data not block aligned"
    );
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut out = data.to_vec();
    for chunk in out.as_chunks_mut::<BLOCK_SIZE>().0 {
        cipher.decrypt_block(GenericArray::from_mut_slice(chunk));
    }
    out
}

//! Wii partitions: 32 KiB blocks of 1 KiB of hashes and 31 KiB of data, in
//! 2 MiB groups of 64 blocks hashed together, all encrypted with the
//! partition's title key.

use rayon::prelude::*;
use sha1::{Digest, Sha1};

use crate::crypto::{Key, cbc_decrypt, cbc_encrypt};

pub const BLOCK_HEADER_SIZE: u64 = 0x400;
pub const BLOCK_DATA_SIZE: u64 = 0x7C00;
pub const BLOCK_TOTAL_SIZE: u64 = BLOCK_HEADER_SIZE + BLOCK_DATA_SIZE;
pub const BLOCKS_PER_GROUP: u64 = 0x40;
pub const GROUP_DATA_SIZE: u64 = BLOCK_DATA_SIZE * BLOCKS_PER_GROUP;
pub const GROUP_TOTAL_SIZE: u64 = BLOCK_TOTAL_SIZE * BLOCKS_PER_GROUP;

pub const DIGEST_SIZE: usize = 20;

/// A block's hashes: 31 H0, padding, 8 H1, padding, 8 H2, padding.
pub type HashBlock = [u8; BLOCK_HEADER_SIZE as usize];
/// A block's data.
pub type BlockData = [u8; BLOCK_DATA_SIZE as usize];

/// Where each part of a [`HashBlock`] lies, and its size.
pub const H0: (usize, usize) = (0x000, 31 * DIGEST_SIZE);
pub const PADDING_0: (usize, usize) = (0x26C, 20);
pub const H1: (usize, usize) = (0x280, 8 * DIGEST_SIZE);
pub const PADDING_1: (usize, usize) = (0x320, 32);
pub const H2: (usize, usize) = (0x340, 8 * DIGEST_SIZE);
pub const PADDING_2: (usize, usize) = (0x3E0, 32);

fn sha1(data: &[u8]) -> [u8; DIGEST_SIZE] {
    Sha1::digest(data).into()
}

/// Hash a group's 64 blocks of data as a Wii disc does, `data` holding as
/// many blocks as there are, zeros for the rest.
pub fn hash_group(data: &[BlockData]) -> Vec<HashBlock> {
    debug_assert_eq!(data.len(), BLOCKS_PER_GROUP as usize);
    // H0: each KiB of a block's data
    let mut hashes: Vec<HashBlock> = data
        .par_iter()
        .map(|block| {
            let mut hashes = [0; BLOCK_HEADER_SIZE as usize];
            for (j, kib) in block.chunks(0x400).enumerate() {
                hashes[j * DIGEST_SIZE..][..DIGEST_SIZE].copy_from_slice(&sha1(kib));
            }
            hashes
        })
        .collect();
    // H1: each block's H0, shared by its 8 neighbours
    let h1: Vec<[u8; DIGEST_SIZE]> = hashes.iter().map(|block| sha1(&block[..H0.1])).collect();
    for (i, block) in hashes.iter_mut().enumerate() {
        let base = i / 8 * 8;
        for j in 0..8 {
            block[H1.0 + j * DIGEST_SIZE..][..DIGEST_SIZE].copy_from_slice(&h1[base + j]);
        }
    }
    // H2: each 8 blocks' H1, shared by all 64
    let h2: Vec<[u8; DIGEST_SIZE]> = (0..8)
        .map(|k| sha1(&hashes[k * 8][H1.0..H1.0 + H1.1]))
        .collect();
    for block in &mut hashes {
        for (k, hash) in h2.iter().enumerate() {
            block[H2.0 + k * DIGEST_SIZE..][..DIGEST_SIZE].copy_from_slice(hash);
        }
    }
    hashes
}

/// Encrypt a block from its hashes and data: the hashes with an IV of
/// zeros, the data chained from the encrypted hashes.
pub fn encrypt_block(key: &Key, hashes: &HashBlock, data: &BlockData, out: &mut [u8]) {
    let (header, body) = out.split_at_mut(BLOCK_HEADER_SIZE as usize);
    header.copy_from_slice(hashes);
    cbc_encrypt(key, &[0; 16], header);
    let iv: [u8; 16] = header[0x3D0..0x3E0].try_into().unwrap();
    body.copy_from_slice(data);
    cbc_encrypt(key, &iv, body);
}

/// Decrypt a block's hashes.
pub fn decrypt_hashes(key: &Key, block: &[u8]) -> HashBlock {
    let mut hashes: HashBlock = block[..BLOCK_HEADER_SIZE as usize].try_into().unwrap();
    cbc_decrypt(key, &[0; 16], &mut hashes);
    hashes
}

/// Decrypt a block's data into `out`.
pub fn decrypt_data(key: &Key, block: &[u8], out: &mut [u8]) {
    let iv: [u8; 16] = block[0x3D0..0x3E0].try_into().unwrap();
    out.copy_from_slice(&block[BLOCK_HEADER_SIZE as usize..BLOCK_TOTAL_SIZE as usize]);
    cbc_decrypt(key, &iv, out);
}

/// Encrypt a hashed group, block by block, into `out`, which holds all 64.
pub fn encrypt_group(key: &Key, hashes: &[HashBlock], data: &[BlockData], out: &mut [u8]) {
    out.par_chunks_mut(BLOCK_TOTAL_SIZE as usize)
        .zip(hashes.par_iter().zip(data.par_iter()))
        .for_each(|(block, (hashes, data))| encrypt_block(key, hashes, data, block));
}

/// A partition's ticket, at its very start.
pub const TICKET_SIZE: usize = 0x2A4;
const TICKET_TITLE_KEY: usize = 0x1BF;
const TICKET_TITLE_ID: usize = 0x1DC;

/// Decrypt a ticket's title key with a common key.
pub fn title_key(ticket: &[u8], common_key: &Key) -> Key {
    let mut key: Key = ticket[TICKET_TITLE_KEY..TICKET_TITLE_KEY + 16]
        .try_into()
        .unwrap();
    let mut iv = [0; 16];
    iv[..8].copy_from_slice(&ticket[TICKET_TITLE_ID..TICKET_TITLE_ID + 8]);
    cbc_decrypt(common_key, &iv, &mut key);
    key
}

/// Whether `key` decrypts `block`, a partition's block, to data that matches
/// its own first hash.
pub fn key_fits(key: &Key, block: &[u8]) -> bool {
    let hashes = decrypt_hashes(key, block);
    let mut data = [0; BLOCK_DATA_SIZE as usize];
    decrypt_data(key, block, &mut data);
    hashes[..DIGEST_SIZE] == sha1(&data[..0x400])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_decrypts_to_what_was_encrypted() {
        let key = [7; 16];
        let mut hashes = [0; BLOCK_HEADER_SIZE as usize];
        hashes
            .iter_mut()
            .enumerate()
            .for_each(|(i, b)| *b = i as u8);
        let mut data = [0; BLOCK_DATA_SIZE as usize];
        data.iter_mut()
            .enumerate()
            .for_each(|(i, b)| *b = (i * 3) as u8);
        let mut block = vec![0; BLOCK_TOTAL_SIZE as usize];
        encrypt_block(&key, &hashes, &data, &mut block);
        assert_eq!(decrypt_hashes(&key, &block), hashes);
        let mut out = [0; BLOCK_DATA_SIZE as usize];
        decrypt_data(&key, &block, &mut out);
        assert_eq!(out, data);
    }

    #[test]
    fn a_hashed_block_fits_its_key() {
        let key = [9; 16];
        let data = vec![[0x5A; BLOCK_DATA_SIZE as usize]; BLOCKS_PER_GROUP as usize];
        let hashes = hash_group(&data);
        let mut block = vec![0; BLOCK_TOTAL_SIZE as usize];
        encrypt_block(&key, &hashes[0], &data[0], &mut block);
        assert!(key_fits(&key, &block));
        assert!(!key_fits(&[8; 16], &block));
    }
}

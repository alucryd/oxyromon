//! Nintendo AES-XTSN (and XTS) with the little-endian alpha tweak.
//!
//! Port of `nsz.nut.aes128._XTSBase` / `AESXTS` / `AESXTSN`. Nintendo's
//! sector-tweak mapping differs from the standard `aes-xts` crate, so the tweak
//! math (`get_tweak`, `_mul_alpha_le`) is implemented directly here.
//!
//! Structure per 16-byte block `i` of a sector:
//!   T_0 = K2_ecb_encrypt(big_endian(sector))
//!   T_i = mul_alpha_le(T_{i-1})
//!   C_i = K1_ecb(P_i ^ T_i) ^ T_i

use crate::crypto::ecb::{self, BLOCK_SIZE};

/// GF(2^128) multiply-by-alpha in little-endian byte order.
///
/// Mirrors `aes128._mul_alpha_le`: interpret the 16 bytes as a little-endian
/// integer, shift left one, reduce with the 0x87 polynomial on carry.
pub fn mul_alpha_le(t: [u8; 16]) -> [u8; 16] {
    let mut x = u128::from_le_bytes(t);
    let carry = (x >> 127) & 1;
    x <<= 1;
    if carry == 1 {
        x ^= 0x87;
    }
    x.to_le_bytes()
}

/// The K2 input block for a sector: the 128-bit big-endian encoding of the sector
/// number (matches `get_tweak(sector).to_bytes(16, "big")`).
pub fn tweak_block(sector: u64) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[8..].copy_from_slice(&sector.to_be_bytes());
    b
}

/// XTSN-crypt `buf` in place. `buf.len()` must be a multiple of 16.
///
/// `sector_size` is the sector granularity (0x200 for NCA headers/sections).
/// `start_sector` is the index of the first sector covered by `buf`.
pub fn crypt(
    key1: &[u8; 16],
    key2: &[u8; 16],
    sector_size: usize,
    start_sector: u64,
    buf: &mut [u8],
    decrypt: bool,
) {
    assert!(
        buf.len().is_multiple_of(BLOCK_SIZE),
        "xtsn: buffer not block aligned"
    );
    assert!(
        sector_size > 0 && sector_size.is_multiple_of(BLOCK_SIZE),
        "xtsn: bad sector size"
    );

    let mut sector = start_sector;
    let mut idx = 0;
    while idx < buf.len() {
        let sector_end = (idx + sector_size).min(buf.len());
        // T_0 for this sector
        let mut tweak = tweak_block(sector);
        ecb::encrypt_block(key2, &mut tweak);
        while idx < sector_end {
            let mut block = [0u8; 16];
            for (b, (x, t)) in block.iter_mut().zip(buf[idx..].iter().zip(&tweak)) {
                *b = x ^ t;
            }
            if decrypt {
                ecb::decrypt_block(key1, &mut block);
            } else {
                ecb::encrypt_block(key1, &mut block);
            }
            for (x, (b, t)) in buf[idx..idx + 16].iter_mut().zip(block.iter().zip(&tweak)) {
                *x = b ^ t;
            }
            tweak = mul_alpha_le(tweak);
            idx += BLOCK_SIZE;
        }
        sector += 1;
    }
}

/// Decrypt a buffer with a 32-byte key (key1 = first half, key2 = second half),
/// matching the `AESXTS` convenience wrapper.
pub fn crypt_with_32byte_key(
    key: &[u8; 32],
    sector_size: usize,
    start_sector: u64,
    buf: &mut [u8],
    decrypt: bool,
) {
    let k1: [u8; 16] = key[..16].try_into().unwrap();
    let k2: [u8; 16] = key[16..].try_into().unwrap();
    crypt(&k1, &k2, sector_size, start_sector, buf, decrypt);
}

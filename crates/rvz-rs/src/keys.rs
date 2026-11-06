//! Wii common keys, which turn a partition's ticket into the title key its
//! data is encrypted with. None ship with this crate: they come from a text
//! file, a console's OTP dump, or a BootMii `keys.bin`.
//!
//! Only writing an RVZ from a Wii disc needs one. An RVZ keeps each
//! partition's title key, so reading one, or rewriting it, needs none, and
//! neither does a GameCube disc.

use std::path::Path;

use crate::crypto::Key;
use crate::error::{Error, Result};

/// Parse keys from text: one 32-digit hex key per line, anything after `#`
/// or `;` ignored, and spaces, tabs, dashes and underscores dropped, as Cemu
/// reads its `keys.txt`. Lines that are not a key are skipped.
pub fn parse_keys(text: &str) -> Vec<Key> {
    text.lines()
        .filter_map(|line| {
            let line = line.split(['#', ';']).next().unwrap_or_default();
            let digits: Vec<u8> = line
                .bytes()
                .filter(|b| !matches!(b, b' ' | b'\t' | b'-' | b'_' | b'\r'))
                .collect();
            if digits.len() != 32 {
                return None;
            }
            let mut key = [0; 16];
            for (byte, pair) in key.iter_mut().zip(digits.as_chunks::<2>().0) {
                *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
            }
            Some(key)
        })
        .collect()
}

/// The size of a Wii's OTP.
const WII_OTP_SIZE: usize = 0x80;
/// The size of a Wii U's OTP, `otp.bin`, whose first bank is a Wii's OTP.
const WIIU_OTP_SIZE: usize = 0x400;
/// Where an OTP keeps the Wii common key.
const OTP_COMMON_KEY: usize = 0x14;
/// What a BootMii `keys.bin` starts with, a text header before the OTP.
const BOOTMII_MAGIC: &[u8] = b"BackupMii";
/// Where a BootMii `keys.bin` keeps the OTP.
const BOOTMII_OTP: usize = 0x100;

/// Take the Wii common key from an OTP dump, a Wii's or a Wii U's, leaving
/// it out if it was never programmed (all zeros or all ones).
pub fn parse_otp(otp: &[u8]) -> Vec<Key> {
    otp.get(OTP_COMMON_KEY..OTP_COMMON_KEY + 16)
        .and_then(|key| <Key>::try_from(key).ok())
        .filter(|key| key.iter().any(|&byte| byte != 0) && key.iter().any(|&byte| byte != 0xFF))
        .into_iter()
        .collect()
}

/// Whether `bytes` are a BootMii `keys.bin`.
pub fn is_bootmii(bytes: &[u8]) -> bool {
    bytes.starts_with(BOOTMII_MAGIC) && bytes.len() >= BOOTMII_OTP + WII_OTP_SIZE
}

/// Take the Wii common key from a key file's bytes: text as one key per
/// line, a BootMii `keys.bin`, or a Wii's or a Wii U's OTP dump.
pub fn parse_key_file(bytes: &[u8]) -> Option<Vec<Key>> {
    if is_bootmii(bytes) {
        return Some(parse_otp(&bytes[BOOTMII_OTP..]));
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => Some(parse_keys(text)),
        Err(_) if matches!(bytes.len(), WII_OTP_SIZE | WIIU_OTP_SIZE) => Some(parse_otp(bytes)),
        Err(_) => None,
    }
}

/// Read a keys file: text as one key per line, a BootMii `keys.bin`, or a
/// Wii's or a Wii U's OTP dump.
pub fn load_keys(path: &Path) -> Result<Vec<Key>> {
    parse_key_file(&std::fs::read(path)?).ok_or_else(|| {
        Error::Unsupported(format!(
            "{}: expected keys as text, a BootMii keys.bin, or an otp.bin",
            path.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_text_keys() {
        let keys = parse_keys("# comment\n00112233-44556677 8899aabb_ccddeeff ; title\nnope\n");
        assert_eq!(
            keys,
            [[
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD,
                0xEE, 0xFF
            ]]
        );
    }

    #[test]
    fn reads_the_common_key_from_an_otp() {
        for size in [WII_OTP_SIZE, WIIU_OTP_SIZE] {
            let mut otp = vec![0xAB; size];
            otp[0x14..0x24].copy_from_slice(&[0x42; 16]);
            assert_eq!(parse_key_file(&otp), Some(vec![[0x42; 16]]));
        }
    }

    #[test]
    fn reads_the_common_key_from_a_bootmii_dump() {
        let mut keys = vec![0; 0x400];
        keys[..BOOTMII_MAGIC.len()].copy_from_slice(BOOTMII_MAGIC);
        keys[0x114..0x124].copy_from_slice(&[0x24; 16]);
        assert_eq!(parse_key_file(&keys), Some(vec![[0x24; 16]]));
    }

    #[test]
    fn leaves_out_an_unprogrammed_key() {
        assert!(parse_otp(&[0; WII_OTP_SIZE]).is_empty());
        assert!(parse_otp(&[0xFF; WII_OTP_SIZE]).is_empty());
    }

    #[test]
    fn refuses_other_binaries() {
        assert_eq!(parse_key_file(&[0xFF; 0x123]), None);
    }
}

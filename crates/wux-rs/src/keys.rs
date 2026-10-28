//! Keys, read the way Cemu reads its `keys.txt`, or from a console's OTP dump.
//! None ship with this crate.

use std::path::Path;

use crate::crypto::Key;
use crate::error::{Error, Result};

/// Parse keys in Cemu's `keys.txt` format: one 32-digit hex key per line,
/// anything after `#` or `;` ignored, and spaces, tabs, dashes and
/// underscores dropped. Lines that are not a key are skipped.
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

/// The size of a Wii U's OTP dump, `otp.bin`.
const OTP_SIZE: usize = 0x400;
/// Where the OTP keeps the common keys: the Wii U's, then the vWii's.
const OTP_COMMON_KEYS: [usize; 2] = [0xE0, 0xD0];

/// Take the Wii U and vWii common keys from a Wii U's OTP dump, leaving out
/// any slot that was never programmed (all zeros or all ones).
pub fn parse_otp(otp: &[u8]) -> Vec<Key> {
    OTP_COMMON_KEYS
        .iter()
        .filter_map(|&offset| otp.get(offset..offset + 16)?.try_into().ok())
        .filter(|key: &Key| {
            key.iter().any(|&byte| byte != 0) && key.iter().any(|&byte| byte != 0xFF)
        })
        .collect()
}

/// Read a keys file: text as Cemu's `keys.txt`, or a 1 KiB binary file as an
/// OTP dump. An OTP dump carries no magic, so a 1 KiB binary file is only
/// refused when neither common key slot was ever programmed.
pub fn load_keys(path: &Path) -> Result<Vec<Key>> {
    let unsupported = || {
        Error::Unsupported(format!(
            "{}: expected Cemu's keys.txt (text) or a Wii U's otp.bin (binary, {OTP_SIZE} bytes)",
            path.display()
        ))
    };
    match String::from_utf8(std::fs::read(path)?) {
        Ok(text) => Ok(parse_keys(&text)),
        Err(error) if error.as_bytes().len() == OTP_SIZE => {
            let keys = parse_otp(error.as_bytes());
            if keys.is_empty() {
                return Err(unsupported());
            }
            Ok(keys)
        }
        Err(_) => Err(unsupported()),
    }
}

/// Where Cemu keeps its `keys.txt` when it isn't portable.
#[cfg(feature = "cli")]
pub fn cemu_keys_path() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|dir| dir.join("Cemu").join("keys.txt"))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::*;

    fn load(bytes: &[u8]) -> Result<Vec<Key>> {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(bytes).unwrap();
        load_keys(file.path())
    }

    #[test]
    fn keys_are_read_as_cemu_reads_them() {
        let keys = parse_keys(
            "# a comment\r\n\
             00112233445566778899aabbccddeeff ; Some Game\r\n\
             0011-2233_4455 6677\t8899AABBCCDDEEFF\n\
             not a key\n\
             0011223344\n",
        );
        let key = [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ];
        assert_eq!(keys, vec![key, key]);
    }

    #[test]
    fn otp_dumps_give_their_common_keys() {
        // dummy keys: none ship with this crate
        let mut otp = vec![0x80; OTP_SIZE];
        otp[0xD0..0xE0].copy_from_slice(&[0x11; 16]);
        otp[0xE0..0xF0].copy_from_slice(&[0x22; 16]);
        assert_eq!(load(&otp).unwrap(), vec![[0x22; 16], [0x11; 16]]);

        // a slot never programmed is left out
        otp[0xD0..0xE0].copy_from_slice(&[0; 16]);
        otp[0xE0..0xF0].copy_from_slice(&[0xFF; 16]);
        assert!(parse_otp(&otp).is_empty());
    }

    #[test]
    fn other_binary_files_are_refused() {
        assert!(matches!(load(&[0x80; 16]), Err(Error::Unsupported(_))));
        // 1 KiB, but no common key slot was ever programmed
        assert!(matches!(
            load(&[0xFF; OTP_SIZE]),
            Err(Error::Unsupported(_))
        ));
    }
}

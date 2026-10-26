//! Keys, read the way Cemu reads its `keys.txt`. None ship with this crate.

use std::path::Path;

use crate::crypto::Key;
use crate::error::Result;

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

/// Read and parse a keys file.
pub fn load_keys(path: &Path) -> Result<Vec<Key>> {
    Ok(parse_keys(&std::fs::read_to_string(path)?))
}

/// Where Cemu keeps its `keys.txt` when it isn't portable.
#[cfg(feature = "cli")]
pub fn cemu_keys_path() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|dir| dir.join("Cemu").join("keys.txt"))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

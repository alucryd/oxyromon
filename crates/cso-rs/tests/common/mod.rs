//! Shared fixture builders for the integration tests.
//! Each test crate uses a subset, so nothing here is dead on its own.
#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};

/// Deterministic pseudo-random bytes, so a failure is reproducible.
pub fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut v = Vec::with_capacity(len);
    let mut state = seed | 1;
    while v.len() < len {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        v.push((state >> 32) as u8);
    }
    v
}

/// A raw ISO image mixing compressible and incompressible regions, so the
/// compressor sees every case: all-zero sectors, high-redundancy sectors,
/// incompressible ones, and plain counting patterns.
pub fn write_iso(path: &Path, sectors: usize, seed: u64) -> Vec<u8> {
    const SECTOR: usize = 2048;
    let target = sectors * SECTOR;
    let mut data = Vec::with_capacity(target);
    let mut state = seed | 1;
    let mut i = 0usize;
    while data.len() < target {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let chunk = match (state >> 16) % 4 {
            0 => vec![0u8; SECTOR],
            1 => {
                let unit = b"cso-rs integration fixture region. ".repeat(64);
                unit[..SECTOR].to_vec()
            }
            2 => noise(SECTOR, state),
            _ => (0..SECTOR as u64)
                .map(|b| ((i as u64 + b) % 4) as u8)
                .collect(),
        };
        data.extend_from_slice(&chunk);
        i += 1;
    }
    data.truncate(target);
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(&data).unwrap();
    f.flush().unwrap();
    data
}

/// A raw ISO image of pseudo-text: words drawn with a skewed frequency from a
/// small random vocabulary, like the strings and tables of real dumps. Unlike
/// [`write_iso`]'s regions, this gives compressors many equally good parses, so
/// different trials often tie on size with different bytes, which is what makes
/// the trial order observable.
pub fn write_text_iso(path: &Path, sectors: usize, seed: u64) -> Vec<u8> {
    let mut state = seed | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let vocabulary: Vec<Vec<u8>> = (0..256)
        .map(|_| {
            let len = 2 + (next() % 9) as usize;
            (0..len).map(|_| b'a' + (next() % 26) as u8).collect()
        })
        .collect();
    let target = sectors * 2048;
    let mut data = Vec::with_capacity(target + 16);
    while data.len() < target {
        // Squaring skews the pick toward the start of the vocabulary.
        let r = (next() % 256) as usize;
        data.extend_from_slice(&vocabulary[r * r / 256]);
        data.push(if next() % 8 == 0 { b'\n' } else { b' ' });
    }
    data.truncate(target);
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(&data).unwrap();
    data
}

/// Locate the reference maxcso binary through `$MAXCSO`, then `$PATH`, or
/// `None` when it is not available.
pub fn maxcso_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("MAXCSO").map(PathBuf::from) {
        return path.is_file().then_some(path);
    }
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join("maxcso"))
        .find(|path| path.is_file())
}

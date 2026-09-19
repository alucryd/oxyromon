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

/// Locate the reference maxcso binary, or `None` when it is not available.
/// Point `MAXCSO` at a build to enable the interop tests.
pub fn maxcso_binary() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("MAXCSO") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    let candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../maxcso/maxcso");
    if candidate.exists() {
        return Some(candidate);
    }
    None
}

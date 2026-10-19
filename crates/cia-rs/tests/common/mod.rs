//! Synthetic CIA builders for the integration tests: enough format to place
//! bytes exactly where the container arithmetic says they should be.
//! Each test uses a subset, so nothing here is dead on its own.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

pub const HEADER_SIZE: usize = 0x2020;

pub fn align(value: usize, alignment: usize) -> usize {
    value.div_ceil(alignment) * alignment
}

/// One content record as it goes into a synthetic TMD.
#[derive(Clone)]
pub struct Rec {
    pub id: u32,
    pub index: u16,
    pub kind: u16,
    pub size: u64,
    pub hash: [u8; 32],
}

/// A record for `data`, hashed the way a TMD hashes the bytes it covers.
pub fn rec(id: u32, index: u16, data: &[u8]) -> Rec {
    Rec {
        id,
        index,
        kind: 0,
        size: data.len() as u64,
        hash: Sha256::digest(data).into(),
    }
}

/// A CTR v1 TMD: RSA-2048/SHA-256 signature, version 1, one CMD group
/// covering every record, both group hashes computed for real.
pub fn tmd(title_id: u64, title_version: u16, records: &[Rec]) -> Vec<u8> {
    let sig = 384;
    let mut data = vec![0u8; sig + 100 + 2336 + records.len() * 48];
    data[..4].copy_from_slice(&0x0001_0004u32.to_be_bytes());
    let issuer = b"Root-CA00000003-CP0000000b";
    data[sig - issuer.len()..sig].copy_from_slice(issuer);
    data[sig] = 1;
    data[sig + 0x0C..sig + 0x14].copy_from_slice(&title_id.to_be_bytes());
    data[sig + 0x5C..sig + 0x5E].copy_from_slice(&title_version.to_be_bytes());
    data[sig + 0x5E..sig + 0x60].copy_from_slice(&(records.len() as u16).to_be_bytes());

    for (at, record) in records.iter().enumerate() {
        let at = sig + 2436 + at * 48;
        data[at..at + 4].copy_from_slice(&record.id.to_be_bytes());
        data[at + 4..at + 6].copy_from_slice(&record.index.to_be_bytes());
        data[at + 6..at + 8].copy_from_slice(&record.kind.to_be_bytes());
        data[at + 8..at + 16].copy_from_slice(&record.size.to_be_bytes());
        data[at + 16..at + 48].copy_from_slice(&record.hash);
    }

    let groups = sig + 100 + 0x20;
    if !records.is_empty() {
        data[groups..groups + 2].copy_from_slice(&0u16.to_be_bytes());
        data[groups + 2..groups + 4].copy_from_slice(&(records.len() as u16).to_be_bytes());
        let covered = Sha256::digest(&data[sig + 2436..sig + 2436 + records.len() * 48]).to_vec();
        data[groups + 4..groups + 36].copy_from_slice(&covered);
    }
    let array = Sha256::digest(&data[groups..groups + 64 * 36]).to_vec();
    data[sig + 100..sig + 132].copy_from_slice(&array);
    data
}

/// The sections of a synthetic CIA, in file order.
#[derive(Default)]
pub struct Sections {
    pub certs: Vec<u8>,
    pub tik: Vec<u8>,
    pub tmd: Vec<u8>,
    /// Contents as they pack into the content section, in order.
    pub contents: Vec<Vec<u8>>,
    pub footer: Vec<u8>,
}

/// Assemble a CIA: the header names the sizes, `bits` are the present
/// content indices, and every non-empty section starts on a 64-byte
/// boundary.
pub fn cia(version: u16, bits: &[u16], sections: &Sections) -> Vec<u8> {
    let mut content = Vec::new();
    let mut running = 0usize;
    for part in &sections.contents {
        running = align(running, 16);
        while content.len() < running {
            content.push(0);
        }
        content.extend_from_slice(part);
        running = content.len();
    }

    let mut header = vec![0u8; HEADER_SIZE];
    header[0x00..0x04].copy_from_slice(&(HEADER_SIZE as u32).to_le_bytes());
    header[0x06..0x08].copy_from_slice(&version.to_le_bytes());
    header[0x08..0x0C].copy_from_slice(&(sections.certs.len() as u32).to_le_bytes());
    header[0x0C..0x10].copy_from_slice(&(sections.tik.len() as u32).to_le_bytes());
    header[0x10..0x14].copy_from_slice(&(sections.tmd.len() as u32).to_le_bytes());
    header[0x14..0x18].copy_from_slice(&(sections.footer.len() as u32).to_le_bytes());
    header[0x18..0x20].copy_from_slice(&(content.len() as u64).to_le_bytes());
    for bit in bits {
        header[0x20 + *bit as usize / 8] |= 1 << (7 - *bit as usize % 8);
    }

    let mut file = header;
    for part in [
        &sections.certs,
        &sections.tik,
        &sections.tmd,
        &content,
        &sections.footer,
    ] {
        if part.is_empty() {
            continue;
        }
        while !file.len().is_multiple_of(64) {
            file.push(0);
        }
        file.extend_from_slice(part);
    }
    file
}

pub fn put32(data: &mut [u8], at: usize, value: u32) {
    data[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

pub fn put16(data: &mut [u8], at: usize, value: u16) {
    data[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

/// Write `data` to a CIA-named temporary file and return its path.
pub fn write_cia(dir: &Path, data: &[u8]) -> PathBuf {
    let path = dir.join("test.cia");
    std::fs::write(&path, data).unwrap();
    path
}

pub fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/Test Game (USA, Europe).cia")
}

/// The reference tool, through `$CTRTOOL` or `$PATH`; `None` when neither
/// has it, which is when the interop tests skip.
pub fn ctrtool_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CTRTOOL").map(PathBuf::from) {
        return Some(path);
    }
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join("ctrtool");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

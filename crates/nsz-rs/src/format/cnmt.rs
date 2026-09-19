//! CNMT (Control Meta / NCA metadata) content entries.
//!
//! Mirrors `nsz/Fs/Cnmt.py`. A CNMT lives in a CNMT-type NCA's first section
//! (already decrypted by the time we read it). Each content entry carries the
//! SHA-256 of one content NCA, used to verify decompressed output.

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct ContentEntry {
    /// 32-byte SHA-256 of the content NCA.
    pub hash: [u8; 32],
    /// 16-byte content id (hex string in Python).
    pub nca_id: [u8; 16],
    pub size: u64,
    pub content_type: u8,
}

#[derive(Debug, Clone)]
pub struct Cnmt {
    pub title_id: String,
    pub version: u32,
    pub title_type: i8,
    pub header_offset: u16,
    pub content_entries: Vec<ContentEntry>,
}

impl Cnmt {
    /// Parse a CNMT from a decrypted buffer (the section content).
    pub fn parse(buf: &[u8]) -> Result<Cnmt> {
        if buf.len() < 0x20 {
            return Err(Error::Corrupt("cnmt too short".into()));
        }
        // titleId = read(8)[::-1] hex
        let title_id = hex::encode(buf[0..8].iter().rev().copied().collect::<Vec<u8>>());
        let version = u32::from_le_bytes(buf[8..12].try_into().unwrap());
        let title_type = buf[12] as i8;
        // buf[13] junk
        let header_offset = u16::from_le_bytes(buf[14..16].try_into().unwrap());
        let content_entry_count = u16::from_le_bytes(buf[16..18].try_into().unwrap()) as usize;
        // metaEntryCount at 18..20 (ignored)

        let base = 0x20 + header_offset as usize;
        let need = base + content_entry_count * 56;
        if buf.len() < need {
            return Err(Error::Corrupt("cnmt content table truncated".into()));
        }
        let mut content_entries = Vec::with_capacity(content_entry_count);
        for i in 0..content_entry_count {
            let b = base + i * 56;
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&buf[b..b + 32]);
            let mut nca_id = [0u8; 16];
            nca_id.copy_from_slice(&buf[b + 32..b + 48]);
            // size = readInt48 (6 bytes LE)
            let mut size_bytes = [0u8; 8];
            size_bytes[..6].copy_from_slice(&buf[b + 48..b + 54]);
            let size = u64::from_le_bytes(size_bytes);
            let content_type = buf[b + 54];
            // buf[b+55] junk
            content_entries.push(ContentEntry {
                hash,
                nca_id,
                size,
                content_type,
            });
        }
        Ok(Cnmt {
            title_id,
            version,
            title_type,
            header_offset,
            content_entries,
        })
    }

    /// Find the content entry whose hash equals `sha256_hex` (case-insensitive).
    pub fn find_by_hash_hex(&self, sha256_hex: &str) -> Option<&ContentEntry> {
        let lower = sha256_hex.to_lowercase();
        self.content_entries
            .iter()
            .find(|e| hex::encode(e.hash) == lower)
    }
}

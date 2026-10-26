//! A title as Nintendo's servers (NUS) deliver it, which is how updates and
//! DLC come: a folder holding a signed TMD listing the contents, a ticket, and
//! one encrypted file per content, the first being the FST.
//!
//! Downloaders name them `title.tmd`, `title.tik` and `<content id>.app`;
//! No-Intro keeps the CDN's names, `tmd.<version>`, `cetk.<version>` and a
//! bare `<content id>`. Both are read.

use std::fs::File;
use std::path::{Path, PathBuf};

use crate::disc::Source;
use crate::error::{Result, corrupt};
use crate::io::read_at;

/// A content's flag: its file is made of hashed blocks.
pub const HASHED: u16 = 0x0002;
/// A content's flag: its hash is SHA-1, rather than SHA-256.
pub const SHA1: u16 = 0x2000;

/// A content the TMD lists.
pub struct Content {
    pub id: u32,
    pub index: u16,
    pub flags: u16,
    pub size: u64,
    /// The hash of the decrypted content, or for a hashed one of its H3
    /// hashes; SHA-1 in its first 20 bytes when `flags` has [`SHA1`].
    pub hash: [u8; 32],
}

/// A Wii U TMD.
pub struct Tmd {
    pub title_id: u64,
    pub version: u16,
    pub contents: Vec<Content>,
}

/// Parse a TMD signed with RSA-2048, as every Wii U title's is.
pub fn parse_tmd(bytes: &[u8]) -> Result<Tmd> {
    const RECORDS: usize = 0xB04;
    if bytes.len() < RECORDS {
        return Err(corrupt("the TMD is cut short"));
    }
    let count = u16::from_be_bytes([bytes[0x1DE], bytes[0x1DF]]) as usize;
    let records = bytes
        .get(RECORDS..RECORDS + count * 0x30)
        .ok_or_else(|| corrupt("the TMD is cut short"))?;
    Ok(Tmd {
        title_id: u64::from_be_bytes(bytes[0x18C..0x194].try_into().unwrap()),
        version: u16::from_be_bytes([bytes[0x1DC], bytes[0x1DD]]),
        contents: records
            .as_chunks::<0x30>()
            .0
            .iter()
            .map(|r| Content {
                id: u32::from_be_bytes(r[..4].try_into().unwrap()),
                index: u16::from_be_bytes([r[4], r[5]]),
                flags: u16::from_be_bytes([r[6], r[7]]),
                size: u64::from_be_bytes(r[8..16].try_into().unwrap()),
                hash: r[16..48].try_into().unwrap(),
            })
            .collect(),
    })
}

/// Parse a Wii TMD, as a vWii title's is: its records are 36 bytes, from
/// 0x1E4, their hash the SHA-1 of the decrypted content. No Wii content is
/// hashed, as [`HASHED`] means it.
pub fn parse_wii_tmd(bytes: &[u8]) -> Result<Tmd> {
    const RECORDS: usize = 0x1E4;
    if bytes.len() < RECORDS {
        return Err(corrupt("the TMD is cut short"));
    }
    let count = u16::from_be_bytes([bytes[0x1DE], bytes[0x1DF]]) as usize;
    let records = bytes
        .get(RECORDS..RECORDS + count * 36)
        .ok_or_else(|| corrupt("the TMD is cut short"))?;
    Ok(Tmd {
        title_id: u64::from_be_bytes(bytes[0x18C..0x194].try_into().unwrap()),
        version: u16::from_be_bytes([bytes[0x1DC], bytes[0x1DD]]),
        contents: records
            .chunks_exact(36)
            .map(|r| {
                let mut hash = [0; 32];
                hash[..20].copy_from_slice(&r[16..36]);
                Content {
                    id: u32::from_be_bytes(r[..4].try_into().unwrap()),
                    index: u16::from_be_bytes([r[4], r[5]]),
                    flags: u16::from_be_bytes([r[6], r[7]]) & !HASHED,
                    size: u64::from_be_bytes(r[8..16].try_into().unwrap()),
                    hash,
                }
            })
            .collect(),
    })
}

/// A NUS title's clusters: each is the `.app` of the content of its index.
pub struct NusSource {
    files: Vec<Option<File>>,
}

impl NusSource {
    /// Open the file of every content `tmd` lists in `dir`.
    pub fn open(dir: &Path, tmd: &Tmd) -> Result<NusSource> {
        let mut files: Vec<Option<File>> = Vec::new();
        for content in &tmd.contents {
            let index = content.index as usize;
            if files.len() <= index {
                files.resize_with(index + 1, || None);
            }
            let app = dir.join(format!("{:08x}.app", content.id));
            let path = match app.is_file() {
                true => app,
                false => dir.join(format!("{:08x}", content.id)),
            };
            files[index] = Some(
                File::open(&path)
                    .map_err(|e| corrupt(format!("the title lacks {}: {e}", path.display())))?,
            );
        }
        Ok(NusSource { files })
    }
}

impl Source for NusSource {
    fn read_cluster(&self, index: u16, _start: u64, offset: u64, buf: &mut [u8]) -> Result<()> {
        let file = self
            .files
            .get(index as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| corrupt(format!("the title has no content {index}")))?;
        // An `.app` may stop short of a whole block: past its end is padding.
        let mut done = 0;
        while done < buf.len() {
            match read_at(file, &mut buf[done..], offset + done as u64)? {
                0 => break,
                n => done += n,
            }
        }
        buf[done..].fill(0);
        Ok(())
    }
}

/// The TMD of the NUS title in `dir`, if it holds one.
pub fn tmd_path(dir: &Path) -> Option<PathBuf> {
    find(dir, "title.tmd", "tmd")
}

/// The ticket of the NUS title in `dir`, if it holds one.
pub fn ticket_path(dir: &Path) -> Option<PathBuf> {
    find(dir, "title.tik", "cetk")
}

/// `dir/<name>`, or else the CDN's `<cdn>` or `<cdn>.<version>`, the highest
/// version when there are several.
fn find(dir: &Path, name: &str, cdn: &str) -> Option<PathBuf> {
    let path = dir.join(name);
    if path.is_file() {
        return Some(path);
    }
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let file_name = entry.file_name().into_string().ok()?;
            let version = match file_name.strip_prefix(cdn)? {
                "" => 0,
                rest => rest.strip_prefix('.')?.parse::<u32>().ok()?,
            };
            entry.path().is_file().then_some((version, entry.path()))
        })
        .max_by_key(|(version, _)| *version)
        .map(|(_, path)| path)
}

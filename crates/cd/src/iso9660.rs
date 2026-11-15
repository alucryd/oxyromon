// SPDX-License-Identifier: BSD-3-Clause
//! The ISO 9660 file system of a data track, with the CD-ROM XA attributes
//! PlayStation discs give every file: which form its sectors are, and
//! whether it is interleaved with others.

use std::collections::HashSet;
use std::io::{Error, ErrorKind, Result};

pub const XA_FORM1: u16 = 0x0800;
pub const XA_FORM2: u16 = 0x1000;
pub const XA_INTERLEAVED: u16 = 0x2000;
pub const XA_CDDA: u16 = 0x4000;
pub const XA_DIRECTORY: u16 = 0x8000;

/// A file or directory, and where it lies.
#[derive(Clone, Debug)]
pub struct Entry {
    pub path: String,
    /// Its first sector, counted from the track's.
    pub lba: u32,
    /// Its size in bytes, 2048 a sector whatever their form.
    pub size: u32,
    pub directory: bool,
    /// Its XA attributes, 0 without.
    pub attributes: u16,
    pub file_number: u8,
}

impl Entry {
    pub fn sectors(&self) -> u32 {
        self.size.div_ceil(2048)
    }
}

fn invalid(what: &str) -> Error {
    Error::new(ErrorKind::InvalidData, format!("ISO 9660: {what}"))
}

fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// Read the entries of the file system whose sectors' 2048 bytes of data
/// `read` gives, sector by number: every directory and file, depth first.
pub fn entries(read: &mut dyn FnMut(u32, &mut [u8; 2048]) -> Result<()>) -> Result<Vec<Entry>> {
    let mut sector = [0; 2048];
    read(16, &mut sector)?;
    if sector[0] != 1 || &sector[1..6] != b"CD001" {
        return Err(invalid("no primary volume descriptor"));
    }
    let root = record(&sector[156..156 + 34]).ok_or_else(|| invalid("a bad root"))?;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = vec![(String::new(), root.1, root.2)];
    while let Some((path, lba, size)) = stack.pop() {
        // Directories that loop are read once
        if !seen.insert(lba) || out.len() > 1 << 20 {
            continue;
        }
        for at in 0..size.div_ceil(2048) {
            read(lba + at, &mut sector)?;
            let mut offset = 0;
            while offset < 2048 && sector[offset] != 0 {
                let len = sector[offset] as usize;
                if len < 34 || offset + len > 2048 {
                    return Err(invalid("a directory record past its sector"));
                }
                let (name, extent, bytes, directory, attributes, file_number) =
                    record(&sector[offset..offset + len]).ok_or_else(|| invalid("a bad record"))?;
                offset += len;
                if name == "\0" || name == "\x01" {
                    continue;
                }
                let name = name.split(';').next().unwrap_or_default();
                let path = format!("{path}/{name}");
                if directory {
                    stack.push((path.clone(), extent, bytes));
                }
                out.push(Entry {
                    path,
                    lba: extent,
                    size: bytes,
                    directory,
                    attributes,
                    file_number,
                });
            }
        }
    }
    Ok(out)
}

/// A directory record: its name, extent, size, whether a directory, and
/// its XA attributes and file number.
fn record(bytes: &[u8]) -> Option<(String, u32, u32, bool, u16, u8)> {
    let name_len = *bytes.get(32)? as usize;
    let name = bytes.get(33..33 + name_len)?;
    let system = bytes
        .get(33 + name_len + (1 - name_len % 2)..)
        .unwrap_or_default();
    let (attributes, file_number) = match system.get(..14) {
        Some(xa) if &xa[6..8] == b"XA" => (u16::from_be_bytes([xa[4], xa[5]]), xa[8]),
        _ => (0, 0),
    };
    Some((
        String::from_utf8_lossy(name).into_owned(),
        le32(bytes, 2),
        le32(bytes, 10),
        bytes[25] & 2 != 0,
        attributes,
        file_number,
    ))
}

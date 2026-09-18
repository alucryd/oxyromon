//! PFS0 (NSP) container: read the file table and build headers.
//!
//! Mirrors `nsz/Fs/Pfs0.py` (`Pfs0Stream.getHeader` / `getStringTableSize`).

use std::io::{Read, Seek, SeekFrom};

use crate::error::{Error, Result};
use crate::format::read_vec;

pub const PFS0_MAGIC: &[u8; 4] = b"PFS0";

#[derive(Debug, Clone)]
pub struct Pfs0Entry {
    pub name: String,
    /// Absolute offset of the file payload in the container.
    pub offset: u64,
    pub size: u64,
    pub flag: u32,
}

/// A parsed PFS0 header over a seekable reader.
pub struct Pfs0Reader<R: Read + Seek> {
    reader: R,
    pub entries: Vec<Pfs0Entry>,
    pub header_size: u64,
}

impl<R: Read + Seek> Pfs0Reader<R> {
    pub fn new(mut reader: R) -> Result<Pfs0Reader<R>> {
        let mut head = [0u8; 16];
        reader.read_exact(&mut head)?;
        if &head[0..4] != PFS0_MAGIC {
            return Err(Error::BadMagic {
                expected: "PFS0".into(),
                found: hex::encode(&head[0..4]),
            });
        }
        let file_count = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
        let string_table_size = u32::from_le_bytes(head[8..12].try_into().unwrap()) as usize;
        // head[12..16] = hashSize (0 for NSP)
        let header_size = 16 + file_count * 24 + string_table_size;

        let table = read_vec(&mut reader, (file_count * 24) as u64)?;
        let string_table = read_vec(&mut reader, string_table_size as u64)?;

        let mut entries = Vec::with_capacity(file_count);
        for i in 0..file_count {
            let base = i * 24;
            let rel_offset = u64::from_le_bytes(table[base..base + 8].try_into().unwrap());
            let size = u64::from_le_bytes(table[base + 8..base + 16].try_into().unwrap());
            let string_offset =
                u32::from_le_bytes(table[base + 16..base + 20].try_into().unwrap()) as usize;
            let flag = u32::from_le_bytes(table[base + 20..base + 24].try_into().unwrap());
            let name = read_cstr(&string_table, string_offset)?;
            let offset = (header_size as u64)
                .checked_add(rel_offset)
                .filter(|off| off.checked_add(size).is_some())
                .ok_or_else(|| Error::Corrupt(format!("{name}: offset out of range")))?;
            entries.push(Pfs0Entry {
                name,
                offset,
                size,
                flag,
            });
        }
        Ok(Pfs0Reader {
            reader,
            entries,
            header_size: header_size as u64,
        })
    }

    /// Read one file's payload into a Vec.
    pub fn read_file(&mut self, entry: &Pfs0Entry) -> Result<Vec<u8>> {
        self.reader.seek(SeekFrom::Start(entry.offset))?;
        Ok(read_vec(&mut self.reader, entry.size)?)
    }

    pub fn into_inner(self) -> R {
        self.reader
    }
}

fn read_cstr(table: &[u8], at: usize) -> Result<String> {
    if at >= table.len() {
        return Err(Error::Corrupt("string offset out of range".into()));
    }
    let end = table[at..]
        .iter()
        .position(|&b| b == 0)
        .map(|p| at + p)
        .unwrap_or(table.len());
    String::from_utf8(table[at..end].to_vec())
        .map_err(|_| Error::Corrupt("invalid utf-8 in string table".into()))
}

/// Compute the padded string-table size for a set of names (mirrors
/// `getStringTableSize` with a fresh `_stringTableSize`).
pub fn padded_string_table_size(names: &[&str]) -> usize {
    let non_padded: usize = names.iter().map(|n| n.len() + 1).sum();
    let header_non_padded = 0x10 + names.len() * 0x18 + non_padded;
    non_padded + align0x20(header_non_padded)
}

/// 0xff => 0x1, 0x100 => 0x20, 0x1ff => 0x1, 0x120 => 0x20
pub fn align0x20(n: usize) -> usize {
    0x20 - (n % 0x20)
}

/// Build a PFS0 header for the given entries (offsets are absolute in the file).
///
/// # Panics
///
/// If `string_table_size` can't hold the names, or an offset precedes the end
/// of the header.
pub fn build_header(entries: &[(String, u64, u64)], string_table_size: usize) -> Vec<u8> {
    let names: Vec<&str> = entries.iter().map(|(n, _, _)| n.as_str()).collect();
    let non_padded: usize = names.iter().map(|n| n.len() + 1).sum();
    let mut string_table = String::new();
    for (i, n) in names.iter().enumerate() {
        if i > 0 {
            string_table.push('\0');
        }
        string_table.push_str(n);
    }
    string_table.push('\0');
    string_table.push_str(&"\0".repeat(string_table_size - non_padded));

    let header_size = 0x10 + entries.len() * 0x18 + string_table_size;
    let mut h = Vec::with_capacity(header_size);
    h.extend_from_slice(PFS0_MAGIC);
    h.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    h.extend_from_slice(&(string_table_size as u32).to_le_bytes());
    h.extend_from_slice(&[0, 0, 0, 0]);
    let mut string_offset = 0u32;
    for (name, offset, size) in entries {
        h.extend_from_slice(&(offset - header_size as u64).to_le_bytes());
        h.extend_from_slice(&size.to_le_bytes());
        h.extend_from_slice(&string_offset.to_le_bytes());
        h.extend_from_slice(&[0, 0, 0, 0]);
        string_offset += name.len() as u32 + 1;
    }
    h.extend_from_slice(string_table.as_bytes());
    h
}

//! NCA header: XTSN decrypt + keyblock unwrap + titleKeyDec resolution.
//!
//! Mirrors `nsz/Fs/Nca.py::NcaHeader.open`. The first 0xC00 bytes of an NCA are
//! AES-XTSN-encrypted with the 32-byte `header_key`; the keyblock at 0x300 is an
//! AES-wrapped titlekey unwrapped with the master-key-derived kek.

use std::io::{Read, Seek, SeekFrom};

use crate::crypto::{ctr, xtsn};
use crate::error::{Error, Result};
use crate::format::ncz::Section;
use crate::keys::Keys;

pub const MEDIA_SIZE: u64 = 0x200;
pub const HEADER_ENCRYPTED_SIZE: usize = 0xC00;

pub const CONTENT_PROGRAM: u8 = 0;
pub const CONTENT_META: u8 = 1;
pub const CONTENT_PUBLIC_DATA: u8 = 5;

/// Section crypto types that are AES-CTR on disk (3 = CTR, 4 = BKTR).
pub fn is_ctr(crypto_type: i64) -> bool {
    crypto_type == 3 || crypto_type == 4
}

/// Offset of section `i`'s FS header inside the decrypted NCA header.
pub fn fs_header_offset(i: usize) -> usize {
    0x400 + i * 0x200
}

/// Section `i`'s CTR counter: `reverse([0]*8 || fs_hdr[0x140:0x148])`
/// (mirrors `BaseFs.__init__`).
pub fn section_counter(decrypted_header: &[u8], i: usize) -> [u8; 16] {
    let fs_hdr = fs_header_offset(i);
    let mut counter = [0u8; 16];
    for (c, b) in counter[..8].iter_mut().zip(
        decrypted_header[fs_hdr + 0x140..fs_hdr + 0x148]
            .iter()
            .rev(),
    ) {
        *c = *b;
    }
    counter
}

#[derive(Debug, Clone, Copy)]
pub struct SectionTableEntry {
    pub media_offset: u32,
    pub media_end_offset: u32,
    pub offset: u64,
    pub end_offset: u64,
}

#[derive(Debug, Clone)]
pub struct NcaHeader {
    pub magic: [u8; 4],
    pub is_game_card: i8,
    pub content_type: u8,
    pub crypto_type: i8,
    pub key_index: i8,
    pub size: u64,
    pub title_id: [u8; 8],
    pub content_index: u32,
    pub sdk_version: u32,
    pub crypto_type2: i8,
    pub rights_id: [u8; 16],
    pub section_tables: [SectionTableEntry; 4],
    pub master_key: i32,
    pub key_block: [u8; 64],
    pub title_key_dec: [u8; 16],
}

/// Decrypt the first 0xC00 bytes of an NCA in place with the header key.
pub fn decrypt_header_bytes(header_key: &[u8; 32], buf: &mut [u8]) {
    assert!(buf.len() >= HEADER_ENCRYPTED_SIZE);
    xtsn::crypt_with_32byte_key(
        header_key,
        0x200,
        0,
        &mut buf[..HEADER_ENCRYPTED_SIZE],
        true,
    );
}

impl NcaHeader {
    /// Parse a decrypted 0xC00-byte NCA header.
    ///
    /// `rights_title_key` is the *encrypted* title key for a rights-managed NCA
    /// (from a ticket / title.keys), looked up by the rights title id. Ignored for
    /// non-rights NCAs (which use `key_block[2]`).
    pub fn parse(
        decrypted: &[u8],
        keys: &Keys,
        rights_title_key: Option<&[u8; 16]>,
    ) -> Result<NcaHeader> {
        if decrypted.len() < HEADER_ENCRYPTED_SIZE {
            return Err(Error::Corrupt("NCA header too short".into()));
        }
        let magic: [u8; 4] = decrypted[0x200..0x204].try_into().unwrap();
        if magic != *b"NCA3" && magic != *b"NCA2" {
            return Err(Error::BadMagic {
                expected: "NCA3/NCA2".into(),
                found: hex::encode(magic),
            });
        }
        let is_game_card = decrypted[0x204] as i8;
        let content_type = decrypted[0x205];
        let crypto_type = decrypted[0x206] as i8;
        let key_index = decrypted[0x207] as i8;
        let size = u64::from_le_bytes(decrypted[0x208..0x210].try_into().unwrap());
        let mut title_id = [0u8; 8];
        title_id.copy_from_slice(&decrypted[0x210..0x218]);
        let content_index = u32::from_le_bytes(decrypted[0x218..0x21C].try_into().unwrap());
        let sdk_version = u32::from_le_bytes(decrypted[0x21C..0x220].try_into().unwrap());
        let crypto_type2 = decrypted[0x220] as i8;
        let mut rights_id = [0u8; 16];
        rights_id.copy_from_slice(&decrypted[0x230..0x240]);

        let mut section_tables = [SectionTableEntry {
            media_offset: 0,
            media_end_offset: 0,
            offset: 0,
            end_offset: 0,
        }; 4];
        for (i, st) in section_tables.iter_mut().enumerate() {
            let base = 0x240 + i * 0x10;
            let media_offset = u32::from_le_bytes(decrypted[base..base + 4].try_into().unwrap());
            let media_end_offset =
                u32::from_le_bytes(decrypted[base + 4..base + 8].try_into().unwrap());
            *st = SectionTableEntry {
                media_offset,
                media_end_offset,
                offset: media_offset as u64 * MEDIA_SIZE,
                end_offset: media_end_offset as u64 * MEDIA_SIZE,
            };
        }

        let mut master_key = (crypto_type.max(crypto_type2) as i32) - 1;
        if master_key < 0 {
            master_key = 0;
        }

        let mut enc_key_block = [0u8; 64];
        enc_key_block.copy_from_slice(&decrypted[0x300..0x340]);
        let mut key_block = [0u8; 64];
        for i in 0..4 {
            let off = i * 0x10;
            let mut w = [0u8; 16];
            w.copy_from_slice(&enc_key_block[off..off + 16]);
            let k = keys.unwrap_title_key(&w, master_key as usize)?;
            key_block[off..off + 16].copy_from_slice(&k);
        }

        let title_key_dec = if has_title_rights(&rights_id) {
            match rights_title_key {
                Some(wrapped) => keys.decrypt_title_key(wrapped, master_key as usize)?,
                None => {
                    return Err(Error::MissingKey(format!(
                        "title key for rights-managed NCA rightsId={}",
                        hex::encode(rights_id).to_uppercase()
                    )))
                }
            }
        } else {
            // key() == keys[2]
            let mut k = [0u8; 16];
            k.copy_from_slice(&key_block[32..48]);
            k
        };

        Ok(NcaHeader {
            magic,
            is_game_card,
            content_type,
            crypto_type,
            key_index,
            size,
            title_id,
            content_index,
            sdk_version,
            crypto_type2,
            rights_id,
            section_tables,
            master_key,
            key_block,
            title_key_dec,
        })
    }

    pub fn has_title_rights(&self) -> bool {
        has_title_rights(&self.rights_id)
    }

    /// Derive the NCZ section list from the decrypted header for compression.
    ///
    /// Each section's data region is `[section_tables[i].offset,
    /// section_tables[i].end_offset)`. The per-section crypto type and counter
    /// live in that section's own FS header at `0x400 + i*0x200` (still inside
    /// the XTS-decrypted 0xC00 region): `fsType = hdr[0x3]`, `cryptoType =
    /// hdr[0x4]`. Sections with `fsType == 0` or no size are skipped. Sections
    /// with a BKTR subsection table (update NCAs) are split per subsection so
    /// each piece decrypts with its own counter (mirrors
    /// `BaseFs.getEncryptionSections`); `nca` is read for that table.
    pub fn encryption_sections<R: Read + Seek>(
        &self,
        decrypted_header: &[u8],
        nca: &mut R,
    ) -> Result<Vec<Section>> {
        let mut sections = Vec::new();
        for (i, st) in self.section_tables.iter().enumerate() {
            let fs_hdr = fs_header_offset(i);
            if st.end_offset <= st.offset || decrypted_header[fs_hdr + 0x3] == 0 {
                continue;
            }
            let section = Section {
                offset: st.offset as i64,
                size: (st.end_offset - st.offset) as i64,
                crypto_type: decrypted_header[fs_hdr + 0x4] as i64,
                crypto_key: self.title_key_dec,
                crypto_counter: section_counter(decrypted_header, i),
            };
            match bktr_subsections(&section, &decrypted_header[fs_hdr..fs_hdr + 0x200], nca)? {
                Some(subs) => sections.extend(subs),
                None => sections.push(section),
            }
        }
        Ok(sections)
    }
}

/// Split a BKTR section along its subsection table. Returns `None` when the
/// section has no table or the table doesn't tile the section cleanly, in which
/// case the section is compressed as one piece (still lossless, just less
/// compressible).
fn bktr_subsections<R: Read + Seek>(
    section: &Section,
    fs_hdr: &[u8],
    nca: &mut R,
) -> Result<Option<Vec<Section>>> {
    const NODE: usize = 0x4000;
    let table_offset = u64::from_le_bytes(fs_hdr[0x120..0x128].try_into().unwrap());
    let table_size = u64::from_le_bytes(fs_hdr[0x128..0x130].try_into().unwrap());
    let section_size = section.size as u64;
    if table_size == 0 || !is_ctr(section.crypto_type) || table_offset >= section_size {
        return Ok(None);
    }

    // The table is a 0x4000 header node followed by 0x4000 bucket nodes, CTR
    // encrypted with the section's base counter.
    let table_len = table_size.min(section_size - table_offset) as usize;
    let abs = section.offset as u64 + table_offset;
    let mut table = vec![0u8; table_len];
    nca.seek(SeekFrom::Start(abs))?;
    nca.read_exact(&mut table)?;
    ctr::keystream_xor(
        &section.crypto_key,
        &section.crypto_counter,
        abs,
        &mut table,
    );
    if table.len() < NODE {
        return Ok(None);
    }
    let bucket_count = u32::from_le_bytes(table[4..8].try_into().unwrap()) as usize;

    let mut subs: Vec<Section> = Vec::new();
    let mut expected = 0u64; // subsections must tile the section from offset 0
    for b in 0..bucket_count {
        let Some(bucket) = table.get(NODE * (b + 1)..NODE * (b + 2)) else {
            return Ok(None);
        };
        let entry_count = u32::from_le_bytes(bucket[4..8].try_into().unwrap()) as usize;
        let end_offset = u64::from_le_bytes(bucket[8..16].try_into().unwrap());
        if entry_count > (NODE - 0x10) / 0x10 {
            return Ok(None);
        }
        for e in 0..entry_count {
            let entry = &bucket[0x10 + e * 0x10..0x20 + e * 0x10];
            let virtual_offset = u64::from_le_bytes(entry[0..8].try_into().unwrap());
            let next = if e + 1 < entry_count {
                u64::from_le_bytes(bucket[0x20 + e * 0x10..0x28 + e * 0x10].try_into().unwrap())
            } else {
                end_offset
            };
            if virtual_offset != expected || next <= virtual_offset || next > section_size {
                return Ok(None);
            }
            // setBktrCounter: bytes 4..8 of the counter carry the entry's ctr value
            // (little-endian on disk, big-endian in the counter).
            let ctr_val = u32::from_le_bytes(entry[12..16].try_into().unwrap());
            let mut counter = section.crypto_counter;
            counter[4..8].copy_from_slice(&ctr_val.to_be_bytes());
            subs.push(Section {
                offset: section.offset + virtual_offset as i64,
                size: (next - virtual_offset) as i64,
                crypto_counter: counter,
                ..section.clone()
            });
            expected = next;
        }
    }
    if subs.is_empty() {
        return Ok(None);
    }
    // The remainder (the BKTR tables themselves) uses the base counter.
    if expected < section_size {
        subs.push(Section {
            offset: section.offset + expected as i64,
            size: (section_size - expected) as i64,
            ..section.clone()
        });
    }
    Ok(Some(subs))
}

fn has_title_rights(rights_id: &[u8; 16]) -> bool {
    // Python: rightsId != b"0"*32 (the hex string of all-zero bytes)
    rights_id.iter().any(|&b| b != 0)
}

//! Builders for synthetic, properly encrypted NCAs / NSPs shared by the tests.
#![allow(dead_code)]

use nsz_rs::crypto::{ctr, ecb, xtsn};
use nsz_rs::format::nca;
use nsz_rs::keys::Keys;

#[path = "../vectors_gen.rs"]
mod v;

pub const HEADER_KEY: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

pub fn build_keys() -> Keys {
    let mut text = String::new();
    for (k, val) in v::SOURCES {
        text.push_str(&format!("{k} = {val}\n"));
    }
    for (rev, val) in v::MASTER_KEYS {
        text.push_str(&format!("master_key_{rev} = {val}\n"));
    }
    text.push_str(&format!("header_key = {HEADER_KEY}\n"));
    Keys::from_str(&text, false).expect("keys parse")
}

pub fn pattern(seed: u8, len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| ((i / 7) as u8).wrapping_mul(seed).wrapping_add(seed))
        .collect()
}

/// One NCA section: plaintext placed at `offset`, CTR-encrypted when
/// `crypto_type` is 3/4. `fs_hdr` patches bytes of its FS header.
pub struct Sec {
    pub offset: u64,
    pub plain: Vec<u8>,
    pub crypto_type: u8,
    pub fs_type: u8,
    pub fs_hdr: Vec<(usize, Vec<u8>)>,
}

impl Sec {
    pub fn new(offset: u64, plain: Vec<u8>, crypto_type: u8) -> Sec {
        Sec {
            offset,
            plain,
            crypto_type,
            fs_type: 3,
            fs_hdr: Vec::new(),
        }
    }
}

/// Build an NCA with master key 0. Non-rights NCAs carry `title_key` in keyblock
/// slot 2; rights-managed ones (non-zero `rights_id`) need a ticket.
pub fn build_nca(
    keys: &Keys,
    content_type: u8,
    rights_id: [u8; 16],
    title_key: [u8; 16],
    secs: &[Sec],
) -> Vec<u8> {
    let mut hdr = vec![0u8; 0xC00];
    hdr[0x200..0x204].copy_from_slice(b"NCA3");
    hdr[0x205] = content_type;
    hdr[0x206] = 1; // cryptoType -> master key 0
    hdr[0x230..0x240].copy_from_slice(&rights_id);
    for slot in 0..4u8 {
        let plain = if slot == 2 {
            title_key
        } else {
            [0x30 + slot; 16]
        };
        let wrapped = keys.wrap_title_key(&plain, 0).unwrap();
        let at = 0x300 + slot as usize * 0x10;
        hdr[at..at + 0x10].copy_from_slice(&wrapped);
    }
    for (i, s) in secs.iter().enumerate() {
        let st = 0x240 + i * 0x10;
        let end = s.offset + s.plain.len() as u64;
        hdr[st..st + 4].copy_from_slice(&((s.offset / 0x200) as u32).to_le_bytes());
        hdr[st + 4..st + 8].copy_from_slice(&((end / 0x200) as u32).to_le_bytes());
        let fs = nca::fs_header_offset(i);
        hdr[fs + 0x3] = s.fs_type;
        hdr[fs + 0x4] = s.crypto_type;
        hdr[fs + 0x140..fs + 0x148].copy_from_slice(&[0x10 + i as u8; 8]);
        for (at, bytes) in &s.fs_hdr {
            hdr[fs + at..fs + at + bytes.len()].copy_from_slice(bytes);
        }
    }

    let mut out = hdr.clone();
    for (i, s) in secs.iter().enumerate() {
        assert!(out.len() as u64 <= s.offset, "sections overlap");
        out.resize(s.offset as usize, 0);
        let mut data = s.plain.clone();
        if nca::is_ctr(s.crypto_type as i64) {
            ctr::keystream_xor(
                &title_key,
                &nca::section_counter(&hdr, i),
                s.offset,
                &mut data,
            );
        }
        out.extend_from_slice(&data);
    }
    let hk: [u8; 32] = hex::decode(HEADER_KEY).unwrap().try_into().unwrap();
    xtsn::crypt_with_32byte_key(&hk, 0x200, 0, &mut out[..0xC00], false);
    out
}

/// A Meta NCA whose first section is a PFS0 holding `meta.cnmt`, listing the
/// SHA-256 of each content NCA.
pub fn build_meta_nca(keys: &Keys, content_hashes: &[[u8; 32]]) -> Vec<u8> {
    let mut cnmt = vec![0u8; 0x20];
    cnmt[16..18].copy_from_slice(&(content_hashes.len() as u16).to_le_bytes());
    for h in content_hashes {
        let mut entry = [0u8; 56];
        entry[..32].copy_from_slice(h);
        cnmt.extend_from_slice(&entry);
    }
    let mut section = build_pfs0(&[("meta.cnmt", &cnmt)]);
    section.resize(section.len().next_multiple_of(0x200), 0);
    let mut sec = Sec::new(0xC00, section, 3);
    sec.fs_type = 2; // PFS0, superblock offset at 0x40 stays 0
    build_nca(keys, nca::CONTENT_META, [0; 16], [0x42; 16], &[sec])
}

/// A common ticket (RSA-2048) granting `title_key` (encrypted with titlekek 0).
pub fn build_ticket(keys: &Keys, rights_id: [u8; 16], title_key: [u8; 16]) -> Vec<u8> {
    let mut tik = vec![0u8; 0x2C0];
    tik[..4].copy_from_slice(&0x010004u32.to_le_bytes());
    let enc = ecb::encrypt_blocks(&keys.title_kek(0).unwrap(), &title_key);
    tik[0x180..0x190].copy_from_slice(&enc);
    tik[0x2A0..0x2B0].copy_from_slice(&rights_id);
    tik
}

/// Build a PFS0 from (name, bytes) pairs.
pub fn build_pfs0(files: &[(&str, &[u8])]) -> Vec<u8> {
    let names: Vec<&str> = files.iter().map(|(n, _)| *n).collect();
    let sts = nsz_rs::format::pfs0::padded_string_table_size(&names);
    let mut cur = (0x10 + files.len() * 0x18 + sts) as u64;
    let mut laid = Vec::new();
    for (n, b) in files {
        laid.push((n.to_string(), cur, b.len() as u64));
        cur += b.len() as u64;
    }
    let mut out = nsz_rs::format::pfs0::build_header(&laid, sts);
    for (_, b) in files {
        out.extend_from_slice(b);
    }
    out
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(data).into()
}

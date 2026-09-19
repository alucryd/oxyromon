//! NCA / NSP compress <-> decompress pipeline tests over synthetic, properly
//! encrypted NCAs (see `common`).

mod common;

use std::io::Cursor;

use common::*;
use nsz_rs::crypto::ctr;
use nsz_rs::decompress::{decompress_ncz, read_ncz_header};
use nsz_rs::format::nca;
use nsz_rs::keys::Keys;
use nsz_rs::pipeline::{compress_nca, compress_nsp, decompress_nsz, Compression, TitleKeys};

const SOLID: Compression = Compression {
    level: 3,
    ldm: false,
    block_size_exponent: None,
};
const BLOCK: Compression = Compression {
    level: 3,
    ldm: false,
    block_size_exponent: Some(14),
};

fn compress(nca: &[u8], keys: &Keys, title_keys: &TitleKeys, c: &Compression) -> Option<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    let written = compress_nca(
        &mut Cursor::new(nca),
        nca.len() as u64,
        keys,
        title_keys,
        c,
        &mut out,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(
        written.is_none(),
        out.get_ref().is_empty(),
        "nothing written when skipped"
    );
    written.map(|_| out.into_inner())
}

fn decompress(ncz: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    decompress_ncz(&mut Cursor::new(ncz), &mut out).unwrap();
    out
}

/// The zstd-decoded body of a solid NCZ, i.e. what the compressor decrypted.
fn solid_body(ncz: &[u8]) -> Vec<u8> {
    let (_, block, payload_start) = read_ncz_header(&mut Cursor::new(ncz)).unwrap();
    assert!(block.is_none());
    zstd::decode_all(&ncz[payload_start as usize..]).unwrap()
}

#[test]
fn nca_roundtrips_and_only_ctr_sections_are_decrypted() {
    let keys = build_keys();
    // Section 0 straddles the 0x4000 verbatim header; section 1 is plaintext.
    let (p0, p1) = (pattern(0x33, 0x4000), pattern(0x44, 0x2000));
    let nca = build_nca(
        &keys,
        nca::CONTENT_PROGRAM,
        [0; 16],
        [0x77; 16],
        &[
            Sec::new(0xC00, p0.clone(), 3),
            Sec::new(0x4C00, p1.clone(), 1),
        ],
    );

    let ncz = compress(&nca, &keys, &TitleKeys::new(), &SOLID).unwrap();
    assert!(ncz.len() < nca.len());
    assert_eq!(solid_body(&ncz), [&p0[0x3400..], &p1[..]].concat());
    assert_eq!(decompress(&ncz), nca);

    let ncz = compress(&nca, &keys, &TitleKeys::new(), &BLOCK).unwrap();
    assert_eq!(decompress(&ncz), nca);
}

#[test]
fn ineligible_ncas_are_left_alone() {
    let keys = build_keys();
    let secs = |gap: u64| {
        [
            Sec::new(0x4200, pattern(1, 0x2000), 3),
            Sec::new(0x6200 + gap, pattern(2, 0x2000), 3),
        ]
    };
    // Gap between sections: can't be represented in an NCZ.
    let unpacked = build_nca(
        &keys,
        nca::CONTENT_PROGRAM,
        [0; 16],
        [0x77; 16],
        &secs(0x200),
    );
    assert!(compress(&unpacked, &keys, &TitleKeys::new(), &SOLID).is_none());
    // Control NCAs aren't compressed by nsz.
    let control = build_nca(&keys, 2, [0; 16], [0x77; 16], &secs(0));
    assert!(compress(&control, &keys, &TitleKeys::new(), &SOLID).is_none());
}

#[test]
fn rights_managed_nca_needs_a_title_key() {
    let keys = build_keys();
    let rights_id = [0xAB; 16];
    let nca = build_nca(
        &keys,
        nca::CONTENT_PROGRAM,
        rights_id,
        [0x55; 16],
        &[Sec::new(0x4000, pattern(9, 0x4000), 3)],
    );
    let mut out = Cursor::new(Vec::new());
    let err = compress_nca(
        &mut Cursor::new(&nca),
        nca.len() as u64,
        &keys,
        &TitleKeys::new(),
        &SOLID,
        &mut out,
        &mut |_| {},
    );
    assert!(
        err.is_err(),
        "missing title key must be an error, not a silent copy"
    );
}

/// An update-style NCA: one BKTR section holding `data_len` bytes of data in two
/// subsections (ctr 5 and 6), followed by the 0x8000-byte BKTR table. Returns
/// the NCA and the section's plaintext.
fn build_bktr_nca(keys: &Keys, data_len: u64) -> (Vec<u8>, Vec<u8>) {
    let title_key = [0x66; 16];
    let (base, abs) = (0x4000u64, |rel: u64| 0x4000 + rel);
    let mut table = vec![0u8; 0x8000];
    table[4..8].copy_from_slice(&1u32.to_le_bytes()); // bucket count
    table[0x4004..0x4008].copy_from_slice(&2u32.to_le_bytes()); // entry count
    table[0x4008..0x4010].copy_from_slice(&data_len.to_le_bytes()); // end offset
    table[0x4010..0x4018].copy_from_slice(&0u64.to_le_bytes());
    table[0x401C..0x4020].copy_from_slice(&5u32.to_le_bytes());
    table[0x4020..0x4028].copy_from_slice(&0x4000u64.to_le_bytes());
    table[0x402C..0x4030].copy_from_slice(&6u32.to_le_bytes());
    let plain = [&pattern(0x21, data_len as usize)[..], &table[..]].concat();
    let mut sec = Sec::new(base, plain.clone(), 4);
    sec.fs_hdr = vec![
        (0x120, data_len.to_le_bytes().to_vec()),
        (0x128, 0x8000u64.to_le_bytes().to_vec()),
    ];
    let mut nca = build_nca(keys, nca::CONTENT_PROGRAM, [0; 16], title_key, &[sec]);

    // Re-encrypt each subsection with its own counter instead of the base one.
    let mut base_ctr = [0u8; 16];
    base_ctr[..8].copy_from_slice(&[0x10; 8]);
    for (rel, end, ctr_val) in [(0u64, 0x4000, 5u32), (0x4000, data_len, 6)] {
        let mut sub_ctr = base_ctr;
        sub_ctr[4..8].copy_from_slice(&ctr_val.to_be_bytes());
        let range = abs(rel) as usize..abs(end) as usize;
        ctr::keystream_xor(&title_key, &base_ctr, abs(rel), &mut nca[range.clone()]);
        ctr::keystream_xor(&title_key, &sub_ctr, abs(rel), &mut nca[range]);
    }
    (nca, plain)
}

#[test]
fn bktr_sections_are_split_per_subsection() {
    let keys = build_keys();
    let (nca, plain) = build_bktr_nca(&keys, 0x8000);
    let ncz = compress(&nca, &keys, &TitleKeys::new(), &SOLID).unwrap();
    let (sections, _, _) = read_ncz_header(&mut Cursor::new(&ncz)).unwrap();
    assert_eq!(sections.len(), 3, "two subsections + table remainder");
    assert_eq!(solid_body(&ncz), plain);
    assert_eq!(decompress(&ncz), nca);
}

#[test]
fn progress_follows_compression_not_read_ahead() {
    // The BKTR table at the end of an update NCA is read before compressing;
    // that read must not make the progress jump to (almost) done.
    let keys = build_keys();
    let (nca, _) = build_bktr_nca(&keys, 0x80000);
    let dir = tempfile::tempdir().unwrap();
    let (nsp_path, nsz_path) = (dir.path().join("u.nsp"), dir.path().join("u.nsz"));
    std::fs::write(&nsp_path, build_pfs0(&[("update.nca", &nca)])).unwrap();

    // One thread: blocks are compressed (and reported) two at a time.
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    let mut calls = Vec::new();
    pool.install(|| {
        compress_nsp(
            &nsp_path,
            &nsz_path,
            || Ok(keys.clone()),
            &BLOCK,
            false,
            &mut |n| calls.push(n),
        )
    })
    .unwrap();
    let total: u64 = calls.iter().sum();
    assert_eq!(total, std::fs::metadata(&nsp_path).unwrap().len());
    assert!(
        calls.iter().all(|&n| n < total / 4),
        "no big jump: {calls:x?}"
    );
}

#[test]
fn truncated_ncz_is_an_error() {
    let keys = build_keys();
    let nca = build_nca(
        &keys,
        nca::CONTENT_PROGRAM,
        [0; 16],
        [0x77; 16],
        &[Sec::new(0x4000, pattern(3, 0x8000), 3)],
    );
    for c in [SOLID, BLOCK] {
        let ncz = compress(&nca, &keys, &TitleKeys::new(), &c).unwrap();
        let cut = &ncz[..ncz.len() - 16];
        assert!(decompress_ncz(&mut Cursor::new(cut), &mut Vec::new()).is_err());
    }
}

/// A retail-like NSP: a rights-managed Program NCA, its ticket, and a Meta NCA
/// whose CNMT lists the Program NCA's hash.
fn build_nsp(keys: &Keys, cnmt_hash: Option<[u8; 32]>) -> Vec<u8> {
    let (rights_id, title_key) = ([0xAB; 16], [0x55; 16]);
    let program = build_nca(
        keys,
        nca::CONTENT_PROGRAM,
        rights_id,
        title_key,
        &[
            Sec::new(0x4200, pattern(0x33, 0x2000), 3),
            Sec::new(0x6200, pattern(0x44, 0x3000), 3),
        ],
    );
    let meta = build_meta_nca(keys, &[cnmt_hash.unwrap_or(sha256(&program))]);
    let tik = build_ticket(keys, rights_id, title_key);
    build_pfs0(&[
        ("program.nca", &program),
        ("meta.cnmt.nca", &meta),
        ("ab.tik", &tik),
    ])
}

#[test]
fn nsp_roundtrips_byte_for_byte_with_verification() {
    let keys = build_keys();
    let nsp = build_nsp(&keys, None);
    let dir = tempfile::tempdir().unwrap();
    let (nsp_path, nsz_path, out_path) = (
        dir.path().join("a.nsp"),
        dir.path().join("a.nsz"),
        dir.path().join("b.nsp"),
    );
    std::fs::write(&nsp_path, &nsp).unwrap();

    for c in [SOLID, BLOCK] {
        // Progress reports add up to exactly the input size, both ways.
        let mut read = 0;
        compress_nsp(
            &nsp_path,
            &nsz_path,
            || Ok(keys.clone()),
            &c,
            false,
            &mut |n| read += n,
        )
        .unwrap();
        assert_eq!(read, nsp.len() as u64);
        let nsz = std::fs::read(&nsz_path).unwrap();
        assert!(nsz.len() < nsp.len());
        assert!(nsz.windows(11).any(|w| w == b"program.ncz"));

        let mut read = 0;
        let report = decompress_nsz(
            &nsz_path,
            &out_path,
            || Ok(keys.clone()),
            false,
            true,
            true,
            &mut |n| read += n,
        )
        .unwrap();
        assert_eq!(read, nsz.len() as u64);
        assert_eq!((report.verified, report.corrupted), (1, 0));
        assert_eq!(
            std::fs::read(&out_path).unwrap(),
            nsp,
            "NSP -> NSZ -> NSP is lossless"
        );
    }
}

#[test]
fn failed_decompression_removes_the_output() {
    let keys = build_keys();
    let dir = tempfile::tempdir().unwrap();
    let (nsp_path, nsz_path, out_path) = (
        dir.path().join("a.nsp"),
        dir.path().join("a.nsz"),
        dir.path().join("b.nsp"),
    );

    // Hash mismatch under strict verification.
    std::fs::write(&nsp_path, build_nsp(&keys, Some([0xFF; 32]))).unwrap();
    compress_nsp(
        &nsp_path,
        &nsz_path,
        || Ok(keys.clone()),
        &SOLID,
        true,
        &mut |_| {},
    )
    .unwrap();
    assert!(decompress_nsz(
        &nsz_path,
        &out_path,
        || Ok(keys.clone()),
        true,
        true,
        true,
        &mut |_| {}
    )
    .is_err());
    assert!(!out_path.exists());

    // Truncated container.
    std::fs::write(&nsp_path, build_nsp(&keys, None)).unwrap();
    compress_nsp(
        &nsp_path,
        &nsz_path,
        || Ok(keys.clone()),
        &SOLID,
        true,
        &mut |_| {},
    )
    .unwrap();
    let nsz = std::fs::read(&nsz_path).unwrap();
    std::fs::write(&nsz_path, &nsz[..nsz.len() - 0x100]).unwrap();
    assert!(decompress_nsz(
        &nsz_path,
        &out_path,
        || Ok(keys.clone()),
        true,
        false,
        false,
        &mut |_| {}
    )
    .is_err());
    assert!(!out_path.exists());
}

#[test]
fn merged_nsp_verifies_against_every_cnmt() {
    // Base game + update in one container, each with its own CNMT listing only
    // its own Program NCA.
    let keys = build_keys();
    let program = |seed| {
        build_nca(
            &keys,
            nca::CONTENT_PROGRAM,
            [0; 16],
            [seed; 16],
            &[Sec::new(0x4000, pattern(seed, 0x8000), 3)],
        )
    };
    let (base, update) = (program(0x31), program(0x32));
    let (base_meta, update_meta) = (
        build_meta_nca(&keys, &[sha256(&base)]),
        build_meta_nca(&keys, &[sha256(&update)]),
    );
    let nsp = build_pfs0(&[
        ("base.nca", &base),
        ("base.cnmt.nca", &base_meta),
        ("update.nca", &update),
        ("update.cnmt.nca", &update_meta),
    ]);

    let dir = tempfile::tempdir().unwrap();
    let (nsp_path, nsz_path, out_path) = (
        dir.path().join("a.nsp"),
        dir.path().join("a.nsz"),
        dir.path().join("b.nsp"),
    );
    std::fs::write(&nsp_path, &nsp).unwrap();
    compress_nsp(
        &nsp_path,
        &nsz_path,
        || Ok(keys.clone()),
        &SOLID,
        false,
        &mut |_| {},
    )
    .unwrap();
    let report = decompress_nsz(
        &nsz_path,
        &out_path,
        || Ok(keys.clone()),
        false,
        true,
        true,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!((report.verified, report.corrupted), (2, 0));
    assert_eq!(std::fs::read(&out_path).unwrap(), nsp);
}

#[test]
fn keys_are_only_loaded_when_needed() {
    // No NCAs (like a homebrew NSP): neither direction touches the keys, even
    // with strict verification.
    let nsp = build_pfs0(&[("main", b"code"), ("main.npdm", b"meta")]);
    let dir = tempfile::tempdir().unwrap();
    let (nsp_path, nsz_path, out_path) = (
        dir.path().join("a.nsp"),
        dir.path().join("a.nsz"),
        dir.path().join("b.nsp"),
    );
    std::fs::write(&nsp_path, &nsp).unwrap();
    let no_keys = || -> nsz_rs::error::Result<Keys> { panic!("keys loaded") };
    compress_nsp(&nsp_path, &nsz_path, no_keys, &SOLID, false, &mut |_| {}).unwrap();
    decompress_nsz(
        &nsz_path,
        &out_path,
        no_keys,
        false,
        true,
        true,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(std::fs::read(&out_path).unwrap(), nsp);

    // An NCA does need them, and a failed load fails the run.
    let keys = build_keys();
    std::fs::write(&nsp_path, build_nsp(&keys, None)).unwrap();
    let missing = || Keys::load(dir.path().join("prod.keys"), true);
    let err = compress_nsp(&nsp_path, &nsz_path, missing, &SOLID, false, &mut |_| {}).unwrap_err();
    assert!(err.to_string().contains("prod.keys"), "{err}");
    assert!(!nsz_path.exists());
}

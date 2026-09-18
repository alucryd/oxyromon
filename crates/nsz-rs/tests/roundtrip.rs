//! NCZ compress <-> decompress round-trip with synthetic CTR sections.
//! Verifies the reconstructed NCA bytes and SHA-256 match a hand-computed
//! expectation (header + CTR-re-encrypted sections).

mod common;

use std::io::Cursor;

use common::pattern;

use sha2::{Digest, Sha256};

use nsz_rs::compress::{block_compress_ncz, solid_compress_ncz};
use nsz_rs::crypto::ctr;
use nsz_rs::format::ncz::Section;

fn k16(seed: u8) -> [u8; 16] {
    std::array::from_fn(|i| (i as u8).wrapping_mul(seed).wrapping_add(1))
}

/// Build the expected reconstructed NCA: header || gap || CTR(sec0) || CTR(sec1).
/// (plaintext, key, counter, offset)
type PlainSection<'a> = (&'a [u8], [u8; 16], [u8; 16], i64);

fn expected_nca(header: &[u8], gap: &[u8], secs: &[PlainSection]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(header);
    out.extend_from_slice(gap);
    for (plain, key, counter, offset) in secs {
        let mut enc = plain.to_vec();
        ctr::keystream_xor(key, counter, *offset as u64, &mut enc);
        out.extend_from_slice(&enc);
    }
    out
}

fn run_roundtrip(solid: bool) {
    let header = pattern(0x11, 0x4000);
    let gap = pattern(0x22, 0x1000);
    let sec0_plain = pattern(0x33, 0x2000);
    let sec1_plain = pattern(0x44, 0x3000);

    let k0 = k16(0x51);
    let c0 = k16(0x61);
    let k1 = k16(0x52);
    let c1 = k16(0x62);

    let sections = vec![
        Section {
            offset: 0x5000,
            size: 0x2000,
            crypto_type: 3,
            crypto_key: k0,
            crypto_counter: c0,
        },
        Section {
            offset: 0x7000,
            size: 0x3000,
            crypto_type: 3,
            crypto_key: k1,
            crypto_counter: c1,
        },
    ];

    let mut body = Vec::new();
    body.extend_from_slice(&gap);
    body.extend_from_slice(&sec0_plain);
    body.extend_from_slice(&sec1_plain);

    let mut ncz = Cursor::new(Vec::new());
    if solid {
        solid_compress_ncz(
            &header,
            &sections,
            &body[..],
            12,
            false,
            &mut ncz,
            &mut |_| {},
        )
        .unwrap();
    } else {
        block_compress_ncz(
            &header,
            &sections,
            &body[..],
            body.len() as u64,
            12,
            false,
            16,
            &mut ncz,
            &mut |_| {},
        )
        .unwrap();
    }
    let ncz = ncz.into_inner();

    // Decompress.
    let mut src = Cursor::new(ncz.clone());
    let mut out = Vec::new();
    let (written, hash) = nsz_rs::decompress::decompress_ncz(&mut src, &mut out).unwrap();

    let expected = expected_nca(
        &header,
        &gap,
        &[(&sec0_plain, k0, c0, 0x5000), (&sec1_plain, k1, c1, 0x7000)],
    );

    assert_eq!(out.len(), expected.len(), "nca length (solid={solid})");
    assert_eq!(out, expected, "reconstructed NCA mismatch (solid={solid})");
    let mut h = Sha256::new();
    h.update(&expected);
    assert_eq!(
        hash,
        hex::encode(h.finalize()),
        "sha256 mismatch (solid={solid})"
    );
    assert_eq!(
        written,
        expected.len() as u64,
        "written count (solid={solid})"
    );
}

#[test]
fn solid_roundtrip() {
    run_roundtrip(true);
}

#[test]
fn block_roundtrip() {
    run_roundtrip(false);
}

#[test]
fn ncz_header_roundtrip_preserves_sections() {
    let sections = vec![
        Section {
            offset: 0x4000,
            size: 0x1000,
            crypto_type: 3,
            crypto_key: k16(0x71),
            crypto_counter: k16(0x81),
        },
        Section {
            offset: 0x5000,
            size: 0x2000,
            crypto_type: 4,
            crypto_key: k16(0x72),
            crypto_counter: k16(0x82),
        },
    ];
    let bh = nsz_rs::format::ncz::BlockHeader {
        version: 2,
        block_type: 1,
        unused: 0,
        block_size_exponent: 20,
        number_of_blocks: 3,
        decompressed_size: 0x30000,
        compressed_block_size_list: vec![100, 200, 300],
    };
    let buf = nsz_rs::format::ncz::write_header(&sections, Some(&bh));
    let (secs, block) = nsz_rs::format::ncz::parse_header(&buf).unwrap();
    assert_eq!(secs, sections);
    assert_eq!(block.unwrap(), bh);
}

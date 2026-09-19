//! Crafted / malformed inputs must fail cleanly: no panics, no allocations
//! sized by untrusted header fields.

mod common;

use std::io::Cursor;

use common::{build_keys, build_pfs0};
use nsz_rs::decompress::read_ncz_header;
use nsz_rs::format::pfs0::Pfs0Reader;
use nsz_rs::keys::Keys;
use nsz_rs::pipeline::decompress_nsz;

fn ncz_header(section_count: i64, tail: &[u8]) -> Vec<u8> {
    let mut ncz = vec![0u8; 0x4000];
    ncz.extend_from_slice(b"NCZSECTN");
    ncz.extend_from_slice(&section_count.to_le_bytes());
    ncz.extend_from_slice(tail);
    ncz
}

#[test]
fn huge_ncz_counts_are_rejected() {
    for count in [1i64 << 40, i64::MAX, -1] {
        assert!(read_ncz_header(&mut Cursor::new(ncz_header(count, &[]))).is_err());
    }
    let mut block = b"NCZBLOCK".to_vec();
    block.extend_from_slice(&[2, 1, 0, 20]);
    block.extend_from_slice(&i32::MAX.to_le_bytes());
    block.extend_from_slice(&0i64.to_le_bytes());
    assert!(read_ncz_header(&mut Cursor::new(ncz_header(0, &block))).is_err());
}

#[test]
fn huge_or_overflowing_pfs0_fields_are_rejected() {
    let mut pfs0 = b"PFS0".to_vec();
    pfs0.extend_from_slice(&u32::MAX.to_le_bytes()); // file count
    pfs0.extend_from_slice(&u32::MAX.to_le_bytes()); // string table size
    pfs0.extend_from_slice(&[0; 4]);
    assert!(Pfs0Reader::new(Cursor::new(pfs0)).is_err());

    let mut pfs0 = build_pfs0(&[("a", b"data")]);
    pfs0[0x10..0x18].copy_from_slice(&u64::MAX.to_le_bytes()); // entry offset
    assert!(Pfs0Reader::new(Cursor::new(pfs0)).is_err());
}

#[test]
fn short_key_sources_are_an_error() {
    let text = "titlekek_source = 00112233445566778899aabbccddee\n"; // 15 bytes
    let keys = common::vectors_gen::SOURCES
        .iter()
        .filter(|(k, _)| *k != "titlekek_source")
        .map(|(k, v)| format!("{k} = {v}\n"))
        .collect::<String>()
        + text
        + "master_key_00 = f3fa01080f161d242b323940474e555c\n";
    assert!(Keys::from_str(&keys, false).is_err());
}

#[test]
fn string_table_too_small_for_names_still_decompresses() {
    // One file named "a" whose string table is a single byte (no terminator).
    let mut pfs0 = b"PFS0".to_vec();
    pfs0.extend_from_slice(&1u32.to_le_bytes());
    pfs0.extend_from_slice(&1u32.to_le_bytes());
    pfs0.extend_from_slice(&[0; 4]);
    pfs0.extend_from_slice(&0u64.to_le_bytes());
    pfs0.extend_from_slice(&4u64.to_le_bytes());
    pfs0.extend_from_slice(&[0; 8]);
    pfs0.extend_from_slice(b"aabcd");

    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("a.nsz"), dir.path().join("a.nsp"));
    std::fs::write(&input, &pfs0).unwrap();
    decompress_nsz(
        &input,
        &output,
        || Ok(build_keys()),
        false,
        false,
        false,
        &mut |_| {},
    )
    .unwrap();
    let mut out = Pfs0Reader::new(std::fs::File::open(&output).unwrap()).unwrap();
    let entry = out.entries[0].clone();
    assert_eq!(entry.name, "a");
    assert_eq!(out.read_file(&entry).unwrap(), b"abcd");
}

#[test]
fn missing_cnmt_verification() {
    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("a.nsz"), dir.path().join("a.nsp"));
    let decompress = |files: &[(&str, &[u8])], strict| {
        std::fs::write(&input, build_pfs0(files)).unwrap();
        decompress_nsz(
            &input,
            &output,
            || Ok(build_keys()),
            false,
            true,
            strict,
            &mut |_| {},
        )
    };

    // NCAs but no CNMT: strict fails, non-strict reports them unverified.
    assert!(decompress(&[("x.nca", b"data")], true).is_err());
    let report = decompress(&[("x.nca", b"data")], false).unwrap();
    assert_eq!((report.verified, report.corrupted), (0, 0));
    assert!(!report.files[0].verified);

    // No NCAs: nothing to verify, so even strict succeeds.
    assert!(decompress(&[("x.bin", b"data")], true).is_ok());
}

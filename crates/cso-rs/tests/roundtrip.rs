//! Compress then decompress with this crate alone.

use std::path::Path;

use cso_rs::{CompressOptions, DecompressOptions, Format};

mod common;
use common::write_iso;

fn roundtrip(format: Format, block: u32, sectors: usize, seed: u64) {
    let dir = tempfile::tempdir().unwrap();
    let iso = dir.path().join("in.iso");
    let packed = dir.path().join(format!("out.{}", format.extension()));
    let back = dir.path().join("back.iso");

    let original = write_iso(&iso, sectors, seed);

    let opts = CompressOptions {
        block_size: Some(block),
        threads: 2,
        ..CompressOptions::new(format)
    };
    let stats = cso_rs::compress(&iso, &packed, &opts, &mut |_| {}).unwrap();
    assert_eq!(stats.input_size, original.len() as u64);

    cso_rs::decompress(
        &packed,
        &back,
        &DecompressOptions { threads: 2 },
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(
        std::fs::read(&back).unwrap(),
        original,
        "{format:?} block={block} did not round-trip"
    );

    let header = cso_rs::probe(&packed).unwrap();
    assert_eq!(header.format, format);
    assert_eq!(header.block_size, block);
    assert_eq!(header.uncompressed_size, original.len() as u64);
}

#[test]
fn cso_small() {
    roundtrip(Format::Cso, 2048, 32, 1);
}

#[test]
fn zso_small() {
    roundtrip(Format::Zso, 2048, 32, 2);
}

#[test]
fn cso_large_blocks() {
    roundtrip(Format::Cso, 65536, 512, 3);
}

#[test]
fn zso_large_blocks() {
    roundtrip(Format::Zso, 65536, 512, 4);
}

#[test]
fn cso_max_block() {
    roundtrip(Format::Cso, cso_rs::MAX_BLOCK_SIZE, 256, 5);
}

#[test]
fn single_block_image() {
    roundtrip(Format::Cso, 2048, 1, 6);
    roundtrip(Format::Zso, 2048, 1, 7);
}

#[test]
fn progress_adds_up_to_the_input_size() {
    let dir = tempfile::tempdir().unwrap();
    let iso = dir.path().join("in.iso");
    let packed = dir.path().join("out.zso");
    let back = dir.path().join("back.iso");
    // An odd sector count, so the last block is a partial one.
    let original = write_iso(&iso, 67, 17);
    let options = CompressOptions {
        block_size: Some(8192),
        ..CompressOptions::new(Format::Zso)
    };

    let mut read = 0;
    cso_rs::compress(&iso, &packed, &options, &mut |n| read += n).unwrap();
    assert_eq!(read, original.len() as u64);

    let mut read = 0;
    let options = DecompressOptions::default();
    cso_rs::decompress(&packed, &back, &options, &mut |n| read += n).unwrap();
    assert_eq!(read, std::fs::metadata(&packed).unwrap().len());
}

#[test]
fn a_failed_run_removes_its_output() {
    let dir = tempfile::tempdir().unwrap();
    let iso = dir.path().join("in.iso");
    let packed = dir.path().join("out.cso");
    let back = dir.path().join("back.iso");
    write_iso(&iso, 64, 23);
    cso_rs::compress(
        &iso,
        &packed,
        &CompressOptions::new(Format::Cso),
        &mut |_| {},
    )
    .unwrap();

    // Header and index intact, block data cut short: it fails mid-write.
    let len = std::fs::metadata(&packed).unwrap().len();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&packed)
        .unwrap()
        .set_len(len / 2)
        .unwrap();
    let options = DecompressOptions::default();
    assert!(cso_rs::decompress(&packed, &back, &options, &mut |_| {}).is_err());
    assert!(!back.exists());
}

#[test]
fn a_rejected_input_leaves_an_existing_output_alone() {
    // Validation fails before the output is opened, so a file already there is
    // not ours to delete.
    let dir = tempfile::tempdir().unwrap();
    let iso = dir.path().join("bad.iso");
    let out = dir.path().join("out.cso");
    std::fs::write(&iso, b"not a sector multiple").unwrap();
    std::fs::write(&out, b"keep me").unwrap();
    assert!(cso_rs::compress(&iso, &out, &CompressOptions::new(Format::Cso), &mut |_| {}).is_err());
    assert_eq!(std::fs::read(&out).unwrap(), b"keep me");
}

#[test]
fn rejects_input_that_is_not_sector_aligned() {
    let dir = tempfile::tempdir().unwrap();
    let iso = dir.path().join("bad.iso");
    std::fs::write(&iso, b"not a sector multiple").unwrap();
    let out: &Path = &dir.path().join("out.cso");
    assert!(matches!(
        cso_rs::compress(&iso, out, &CompressOptions::new(Format::Cso), &mut |_| {}),
        Err(cso_rs::Error::InvalidOption(_))
    ));
}

#[test]
fn rejects_bad_block_sizes() {
    let dir = tempfile::tempdir().unwrap();
    let iso = dir.path().join("in.iso");
    write_iso(&iso, 8, 19);
    for bad in [1024u32, 3000, 524288] {
        let opts = CompressOptions {
            format: Format::Cso,
            block_size: Some(bad),
            ..CompressOptions::new(Format::Cso)
        };
        assert!(
            cso_rs::compress(&iso, &dir.path().join("o"), &opts, &mut |_| {}).is_err(),
            "block {bad} should have been rejected"
        );
    }
}

#[test]
fn refuses_to_read_a_non_cso_file() {
    let dir = tempfile::tempdir().unwrap();
    let junk = dir.path().join("junk");
    std::fs::write(
        &junk,
        b"definitely not a CSO or ZSO file at all, no header here",
    )
    .unwrap();
    assert!(cso_rs::probe(&junk).is_err());
    assert!(
        cso_rs::decompress(
            &junk,
            &dir.path().join("o"),
            &DecompressOptions::default(),
            &mut |_| {}
        )
        .is_err()
    );
}

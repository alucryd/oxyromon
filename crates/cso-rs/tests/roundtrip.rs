//! Compress then decompress with this crate alone.

use std::path::Path;

use cso_rs::{CompressOptions, DecompressOptions, Format, Methods};

mod common;
use common::write_iso;

fn roundtrip(format: Format, block: u32, sectors: usize, seed: u64) {
    let dir = tempfile::tempdir().unwrap();
    let iso = dir.path().join("in.iso");
    let packed = dir.path().join(format!("out.{}", format.extension()));
    let back = dir.path().join("back.iso");

    let original = write_iso(&iso, sectors, seed);

    let opts = CompressOptions {
        format,
        block_size: Some(block),
        methods: Methods::default_for(format),
        threads: 2,
        orig_max_cost_percent: 0.0,
        lz4_max_cost_percent: 0.0,
    };
    let stats = cso_rs::compress(&iso, &packed, &opts, &mut |_| {}).unwrap();
    assert_eq!(stats.input_size, original.len() as u64);

    cso_rs::decompress(&packed, &back, &DecompressOptions { threads: 2 }, &mut |_| {})
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
fn every_method_that_fits_the_container() {
    for (format, methods) in [
        (
            Format::Cso,
            Methods {
                zlib: true,
                zlib_brute: true,
                libdeflate: true,
                zopfli: cfg!(feature = "zopfli"),
                ..Methods::default()
            },
        ),
        (
            Format::Zso,
            Methods {
                lz4: true,
                lz4_hc: true,
                lz4_hc_brute: true,
                ..Methods::default()
            },
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let iso = dir.path().join("in.iso");
        let packed = dir.path().join("out");
        let back = dir.path().join("back.iso");
        let original = write_iso(&iso, 64, 11);
        let opts = CompressOptions {
            format,
            block_size: Some(4096),
            methods,
            threads: 4,
            orig_max_cost_percent: 0.0,
            lz4_max_cost_percent: 0.0,
        };
        cso_rs::compress(&iso, &packed, &opts, &mut |_| {}).unwrap();
        cso_rs::decompress(&packed, &back, &DecompressOptions::default(), &mut |_| {}).unwrap();
        assert_eq!(
            std::fs::read(&back).unwrap(),
            original,
            "{format:?} with {methods:?} did not round-trip"
        );
    }
}

#[test]
fn cost_allowances_change_the_layout_but_not_the_result() {
    let dir = tempfile::tempdir().unwrap();
    let iso = dir.path().join("in.iso");
    let original = write_iso(&iso, 128, 13);
    for (orig_cost, lz4_cost) in [(0.0, 0.0), (5.0, 2.0), (25.0, 10.0)] {
        for format in [Format::Cso, Format::Zso] {
            let packed = dir.path().join(format!("{}-{orig_cost}", format.extension()));
            let back = dir.path().join("back.iso");
            let opts = CompressOptions {
                format,
                block_size: Some(2048),
                methods: Methods::default_for(format),
                threads: 2,
                orig_max_cost_percent: orig_cost,
                lz4_max_cost_percent: lz4_cost,
            };
            cso_rs::compress(&iso, &packed, &opts, &mut |_| {}).unwrap();
            cso_rs::decompress(&packed, &back, &DecompressOptions::default(), &mut |_| {}).unwrap();
            assert_eq!(
                std::fs::read(&back).unwrap(),
                original,
                "{format:?} orig_cost={orig_cost} lz4_cost={lz4_cost}"
            );
        }
    }
}

#[test]
fn progress_reaches_the_total() {
    let dir = tempfile::tempdir().unwrap();
    let iso = dir.path().join("in.iso");
    let packed = dir.path().join("out.cso");
    let original = write_iso(&iso, 64, 17);
    let mut last = cso_rs::Progress {
        done: 0,
        total: 0,
        written: 0,
    };
    cso_rs::compress(
        &iso,
        &packed,
        &CompressOptions::new(Format::Cso),
        &mut |p| last = p,
    )
    .unwrap();
    assert_eq!(last.total, original.len() as u64);
    assert_eq!(last.done, last.total);
    assert!(last.written > 0 && last.written <= last.total);
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
fn rejects_a_mismatched_method_set() {
    let dir = tempfile::tempdir().unwrap();
    let iso = dir.path().join("in.iso");
    write_iso(&iso, 8, 21);
    let mixed = Methods {
        zlib: true,
        lz4: true,
        ..Methods::default()
    };
    for format in [Format::Cso, Format::Zso] {
        let opts = CompressOptions {
            format,
            block_size: Some(2048),
            methods: mixed,
            threads: 1,
            orig_max_cost_percent: 0.0,
            lz4_max_cost_percent: 0.0,
        };
        assert!(
            cso_rs::compress(&iso, &dir.path().join("o"), &opts, &mut |_| {}).is_err(),
            "{format:?} with a mixed codec set must be rejected"
        );
    }
}

#[test]
fn refuses_to_read_a_non_cso_file() {
    let dir = tempfile::tempdir().unwrap();
    let junk = dir.path().join("junk");
    std::fs::write(&junk, b"definitely not a CSO or ZSO file at all, no header here").unwrap();
    assert!(cso_rs::probe(&junk).is_err());
    assert!(cso_rs::decompress(
        &junk,
        &dir.path().join("o"),
        &DecompressOptions::default(),
        &mut |_| {}
    )
    .is_err());
}

//! Byte-for-byte agreement with the reference maxcso implementation.
//!
//! Skipped unless a maxcso binary is reachable through `$MAXCSO` or at
//! `../maxcso/maxcso`. These are the tests that matter: a CSO or ZSO is only
//! worth producing if the tools that consume it can read it back.

use std::path::Path;
use std::process::Command;

use cso_rs::{CompressOptions, DecompressOptions, Format, Methods};

mod common;
use common::{maxcso_binary, write_iso};

fn run(bin: &Path, args: &[&str]) {
    let out = Command::new(bin).args(args).output().expect("failed to run maxcso");
    assert!(
        out.status.success(),
        "maxcso {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Our output must decompress, under maxcso, to the original image.
fn maxcso_reads_ours(format: Format, block: u32, seed: u64) {
    let Some(maxcso) = maxcso_binary() else {
        eprintln!("skipping: no maxcso binary found (set $MAXCSO)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let iso = dir.path().join("in.iso");
    let packed = dir.path().join(format!("out.{}", format.extension()));
    let back = dir.path().join("back.iso");

    let original = write_iso(&iso, 256, seed);
    let opts = CompressOptions {
        format,
        block_size: Some(block),
        methods: Methods::default_for(format),
        threads: 4,
        orig_max_cost_percent: 0.0,
        lz4_max_cost_percent: 0.0,
    };
    cso_rs::compress(&iso, &packed, &opts, &mut |_| {}).unwrap();

    run(&maxcso, &["--decompress", path(&packed), "-o", path(&back)]);
    assert_eq!(
        std::fs::read(&back).unwrap(),
        original,
        "maxcso could not reproduce the image from our {format:?} (block {block})"
    );
}

/// maxcso's output must decompress, under us, to the original image.
fn we_read_maxcos(format: Format, maxcso_format: &str, block: u32, seed: u64) {
    let Some(maxcso) = maxcso_binary() else {
        eprintln!("skipping: no maxcso binary found (set $MAXCSO)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let iso = dir.path().join("in.iso");
    let packed = dir.path().join(format!("ref.{}", format.extension()));
    let back = dir.path().join("back.iso");

    let original = write_iso(&iso, 256, seed);
    run(
        &maxcso,
        &[
            &format!("--format={maxcso_format}"),
            &format!("--block={block}"),
            path(&iso),
            "-o",
            path(&packed),
        ],
    );

    cso_rs::decompress(&packed, &back, &DecompressOptions { threads: 4 }, &mut |_| {}).unwrap();
    assert_eq!(
        std::fs::read(&back).unwrap(),
        original,
        "we could not reproduce the image from maxcso's {format:?} (block {block})"
    );
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn maxcso_reads_our_cso() {
    maxcso_reads_ours(Format::Cso, 2048, 101);
}

#[test]
fn maxcso_reads_our_zso() {
    maxcso_reads_ours(Format::Zso, 2048, 102);
}

#[test]
fn maxcso_reads_our_cso_large_blocks() {
    maxcso_reads_ours(Format::Cso, 65536, 103);
}

#[test]
fn maxcso_reads_our_zso_large_blocks() {
    maxcso_reads_ours(Format::Zso, 65536, 104);
}

#[test]
fn we_read_maxcos_cso() {
    we_read_maxcos(Format::Cso, "cso1", 2048, 201);
}

#[test]
fn we_read_maxcos_zso() {
    we_read_maxcos(Format::Zso, "zso", 2048, 202);
}

#[test]
fn we_read_maxcos_cso_large_blocks() {
    we_read_maxcos(Format::Cso, "cso1", 65536, 203);
}

#[test]
fn we_read_maxcos_zso_large_blocks() {
    we_read_maxcos(Format::Zso, "zso", 65536, 204);
}

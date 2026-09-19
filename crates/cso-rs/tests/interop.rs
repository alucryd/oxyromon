//! Agreement with the reference maxcso implementation: each tool reads the
//! other's output back to the original image, and our CSO is byte-identical to
//! maxcso's with the same zlib trials.
//!
//! Skipped unless a maxcso binary is reachable through `$MAXCSO` or `$PATH`.
//! These are the tests that matter: a CSO or ZSO is only worth producing if the
//! tools that consume it can read it back.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::Command;

use cso_rs::{CompressOptions, DecompressOptions, Format};

mod common;
use common::{maxcso_binary, write_iso, write_text_iso};

fn run(bin: &Path, args: &[&str]) {
    let out = Command::new(bin)
        .args(args)
        .output()
        .expect("failed to run maxcso");
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
        block_size: Some(block),
        threads: 4,
        ..CompressOptions::new(format)
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

    cso_rs::decompress(
        &packed,
        &back,
        &DecompressOptions { threads: 4 },
        &mut |_| {},
    )
    .unwrap();
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

/// Our CSO is maxcso's byte for byte when maxcso runs the same zlib trials:
/// the header, the index, the padding, and which of two equal-sized trials wins
/// a block. (ZSO has no such twin: maxcso cannot run LZ4 HC 16 on its own.)
#[test]
fn same_bytes_as_maxcso_zlib_cso() {
    let Some(maxcso) = maxcso_binary() else {
        eprintln!("skipping: no maxcso binary found (set $MAXCSO)");
        return;
    };
    for block in [2048, 16384] {
        let dir = tempfile::tempdir().unwrap();
        let iso = dir.path().join("in.iso");
        let ours = dir.path().join("ours.cso");
        let theirs = dir.path().join("theirs.cso");

        // Pseudo-text, on which the zlib strategies often tie on size: a change
        // in trial order shows up here.
        write_text_iso(&iso, 512, u64::from(block));
        let options = CompressOptions {
            block_size: Some(block),
            ..CompressOptions::new(Format::Cso)
        };
        cso_rs::compress(&iso, &ours, &options, &mut |_| {}).unwrap();
        let block_arg = format!("--block={block}");
        run(
            &maxcso,
            &[
                &block_arg,
                "--format=cso1",
                "--only-zlib",
                path(&iso),
                "-o",
                path(&theirs),
            ],
        );

        assert!(
            std::fs::read(&ours).unwrap() == std::fs::read(&theirs).unwrap(),
            "CSO (block {block}) differs from maxcso --only-zlib"
        );
    }
}

/// Past 2 GiB the index stores offsets shifted right by `index_shift`, and every
/// block is padded to `1 << index_shift`. A sparse image keeps this cheap.
#[test]
#[ignore = "compresses and decompresses a 2 GiB image"]
fn index_shift_round_trips() {
    const SIZE: u64 = (2048 + 4) << 20;
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let iso = dir.path().join("big.iso");
    let packed = dir.path().join("big.zso");

    // Mostly holes, with incompressible and repetitive stretches spread through
    // it so that blocks of every kind land on both sides of the 2 GiB line.
    let mut file = File::create(&iso).unwrap();
    file.set_len(SIZE).unwrap();
    for (i, mib) in [0u64, 700, 1500, 2047, 2050].into_iter().enumerate() {
        file.seek(SeekFrom::Start(mib << 20)).unwrap();
        file.write_all(&common::noise(1 << 20, i as u64 + 1))
            .unwrap();
        file.write_all(&b"cso-rs ".repeat(1 << 17)).unwrap();
    }
    drop(file);

    let options = CompressOptions::new(Format::Zso);
    cso_rs::compress(&iso, &packed, &options, &mut |_| {}).unwrap();
    assert_eq!(cso_rs::probe(&packed).unwrap().index_shift, 1);

    let back = dir.path().join("back.iso");
    cso_rs::decompress(&packed, &back, &DecompressOptions::default(), &mut |_| {}).unwrap();
    assert_same_file(&iso, &back);
    std::fs::remove_file(&back).unwrap();

    if let Some(maxcso) = maxcso_binary() {
        run(&maxcso, &["--decompress", path(&packed), "-o", path(&back)]);
        assert_same_file(&iso, &back);
    }
}

/// Compare two files a chunk at a time, so neither has to fit in memory.
fn assert_same_file(a: &Path, b: &Path) {
    let (mut a, mut b) = (File::open(a).unwrap(), File::open(b).unwrap());
    assert_eq!(a.metadata().unwrap().len(), b.metadata().unwrap().len());
    let (mut x, mut y) = (vec![0u8; 1 << 20], vec![0u8; 1 << 20]);
    let mut offset = 0u64;
    loop {
        let n = a.read(&mut x).unwrap();
        if n == 0 {
            break;
        }
        b.read_exact(&mut y[..n]).unwrap();
        assert!(x[..n] == y[..n], "files differ within the MiB at {offset}");
        offset += n as u64;
    }
}

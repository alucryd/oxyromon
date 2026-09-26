//! Known answers: patches xdelta3 3.2 wrote, over 16 KiB windows so there are
//! several, and the target each must reproduce.

use std::path::{Path, PathBuf};
use tempfile::TempDir;
use xdelta_rs::{Error, Header, decode, read_header};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

fn apply(patch: &str) -> (TempDir, xdelta_rs::Result<Vec<u8>>) {
    let directory = TempDir::new().unwrap();
    let output = directory.path().join("out.bin");
    let result = decode(
        Some(&data("source.bin")),
        &data(patch),
        &output,
        &mut |_| {},
    )
    .map(|()| std::fs::read(&output).unwrap());
    (directory, result)
}

#[test]
fn a_patch_without_secondary_compression_applies() {
    let (_directory, result) = apply("plain.xdelta");
    assert_eq!(result.unwrap(), std::fs::read(data("target.bin")).unwrap());
}

#[test]
fn lzma_sections_decode_across_windows() {
    // Each section kind is one xz stream, continued from window to window.
    let (_directory, result) = apply("lzma.xdelta");
    assert_eq!(result.unwrap(), std::fs::read(data("target.bin")).unwrap());
}

#[test]
fn progress_adds_up_to_the_patch_size() {
    let directory = TempDir::new().unwrap();
    let mut total = 0;
    decode(
        Some(&data("source.bin")),
        &data("lzma.xdelta"),
        &directory.path().join("out.bin"),
        &mut |n| total += n,
    )
    .unwrap();
    assert_eq!(total, std::fs::metadata(data("lzma.xdelta")).unwrap().len());
}

#[test]
fn the_header_names_what_xdelta3_recorded() {
    assert_eq!(
        read_header(&data("lzma.xdelta")).unwrap(),
        Header {
            target_name: Some("target.bin".into()),
            source_name: Some("source.bin".into()),
        }
    );
    assert_eq!(read_header(&data("djw.xdelta")).unwrap(), Header::default());
}

#[test]
fn djw_sections_are_refused() {
    let (directory, result) = apply("djw.xdelta");
    assert!(matches!(result, Err(Error::Unsupported(message)) if message.contains("DJW")));
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn the_wrong_source_fails_its_checksum_and_leaves_nothing() {
    let directory = TempDir::new().unwrap();
    let output = directory.path().join("out.bin");
    std::fs::write(&output, "keep me").unwrap();
    let wrong = directory.path().join("wrong.bin");
    let mut source = std::fs::read(data("source.bin")).unwrap();
    source[100] ^= 0xFF;
    std::fs::write(&wrong, source).unwrap();

    let result = decode(Some(&wrong), &data("plain.xdelta"), &output, &mut |_| {});

    assert!(matches!(result, Err(Error::WrongSource(message)) if message.contains("checksum")));
    assert_eq!(std::fs::read(&output).unwrap(), b"keep me");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
}

#[test]
fn a_missing_source_is_asked_for() {
    let directory = TempDir::new().unwrap();
    let result = decode(
        None,
        &data("plain.xdelta"),
        &directory.path().join("out"),
        &mut |_| {},
    );
    assert!(matches!(result, Err(Error::InvalidOption(_))));
}

#[test]
fn malformed_patches_fail_without_panicking() {
    let directory = TempDir::new().unwrap();
    let patch = std::fs::read(data("lzma.xdelta")).unwrap();
    let output = directory.path().join("out.bin");
    // Every truncation, and a flipped byte at every position.
    for case in 0..patch.len() * 2 {
        let mangled = if case < patch.len() {
            patch[..case].to_vec()
        } else {
            let mut mangled = patch.clone();
            mangled[case - patch.len()] ^= 0x5A;
            mangled
        };
        let path = directory.path().join("mangled.xdelta");
        std::fs::write(&path, mangled).unwrap();
        // Some flips land where nothing checks them; only a panic or a hang fails.
        let _ = decode(Some(&data("source.bin")), &path, &output, &mut |_| {});
    }
    let path = directory.path().join("magic.xdelta");
    std::fs::write(&path, b"PK\x03\x04").unwrap();
    assert!(matches!(read_header(&path), Err(Error::BadMagic { .. })));
}

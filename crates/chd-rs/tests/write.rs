//! Write-path agreement with the reference `chdman` implementation: CHDs we
//! create verify and extract byte-identical through `chdman`, and CHDs
//! `chdman` creates, plain or with a parent, extract byte-identical through
//! `chdrs`. Skipped unless a `chdman` binary is reachable through `$CHDMAN`
//! or `$PATH`.

#![cfg(feature = "cli")]

use std::path::{Path, PathBuf};
use std::process::Command;

/// Locate the reference `chdman` binary through `$CHDMAN`, then `$PATH`, or
/// `None` when it is not available.
fn chdman_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CHDMAN").map(PathBuf::from) {
        return path.is_file().then_some(path);
    }
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join("chdman"))
        .find(|path| path.is_file())
}

fn expect_ok(bin: &Path, args: &[&str]) {
    let out = Command::new(bin)
        .args(args)
        .output()
        .expect("failed to run");
    assert!(
        out.status.success(),
        "{} {:?} failed: {}{}",
        bin.display(),
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Deterministic pseudo-random bytes, xorshift64.
fn fill(data: &mut [u8], seed: u64) {
    let mut state = seed | 1;
    for byte in data {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = (state >> 24) as u8;
    }
}

/// A 256 KiB image of the given flavor, written to `path` and returned.
fn write_image(path: &Path, kind: &str) -> Vec<u8> {
    let mut block = vec![0; 512];
    let image = match kind {
        "zeros" => vec![0; 256 * 1024],
        "pattern" => {
            fill(&mut block, 7);
            block.repeat(512)
        }
        _ => {
            fill(&mut block, 42);
            let mut image = block.repeat(512);
            // Sprinkle real entropy so not every hunk is compressible.
            fill(&mut image[64 * 1024..128 * 1024], 43);
            image
        }
    };
    std::fs::write(path, &image).unwrap();
    image
}

#[test]
fn chdman_reads_what_we_write() {
    let Some(chdman) = chdman_binary() else {
        eprintln!("skipping: no chdman binary found (set $CHDMAN)");
        return;
    };
    for kind in ["zeros", "pattern", "random"] {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("image.bin");
        let data = write_image(&image, kind);
        expect_ok(
            Path::new(env!("CARGO_BIN_EXE_chdrs")),
            &[
                "-o",
                &dir.path().to_string_lossy(),
                "-c",
                "zlib,lzma,huff,flac",
                &image.to_string_lossy(),
            ],
        );
        let chd = dir.path().join("image.chd");
        expect_ok(chdman.as_path(), &["verify", "-i", &chd.to_string_lossy()]);
        let extracted = dir.path().join("extracted.bin");
        expect_ok(
            chdman.as_path(),
            &[
                "extracthd",
                "-i",
                &chd.to_string_lossy(),
                "-o",
                &extracted.to_string_lossy(),
            ],
        );
        assert_eq!(
            std::fs::read(&extracted).unwrap(),
            data,
            "chdman extracted a different image from the {kind} CHD we wrote"
        );
    }
}

#[test]
fn we_read_what_chdman_writes() {
    let Some(chdman) = chdman_binary() else {
        eprintln!("skipping: no chdman binary found (set $CHDMAN)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("image.bin");
    let data = write_image(&image, "random");
    let chd = dir.path().join("image.chd");
    expect_ok(
        chdman.as_path(),
        &[
            "createhd",
            "-i",
            &image.to_string_lossy(),
            "-o",
            &chd.to_string_lossy(),
            "-ss",
            "512",
            "-hs",
            "4096",
        ],
    );
    expect_ok(
        Path::new(env!("CARGO_BIN_EXE_chdrs")),
        &["verify", &chd.to_string_lossy()],
    );
    expect_ok(
        Path::new(env!("CARGO_BIN_EXE_chdrs")),
        &["-o", &dir.path().to_string_lossy(), &chd.to_string_lossy()],
    );
    assert_eq!(
        std::fs::read(dir.path().join("image.iso")).unwrap(),
        data,
        "chdrs extracted a different image from the CHD chdman wrote"
    );
}

/// The base image, its CHD, and a copy with one 64 KiB run replaced, in a
/// fresh temporary directory. The base CHD is written by chdman because its
/// `createhd -op` insists on the CHS metadata chdman stores in the parent.
fn parent_case(chdman: &Path, dir: &Path) -> (PathBuf, PathBuf, Vec<u8>) {
    let base = dir.join("base.bin");
    write_image(&base, "random");
    let base_chd = dir.join("base.chd");
    expect_ok(
        chdman,
        &[
            "createhd",
            "-i",
            &base.to_string_lossy(),
            "-o",
            &base_chd.to_string_lossy(),
            "-ss",
            "512",
            "-hs",
            "4096",
        ],
    );
    let mut modified = std::fs::read(&base).unwrap();
    fill(&mut modified[64 * 1024..128 * 1024], 99);
    let modified_image = dir.join("modified.bin");
    std::fs::write(&modified_image, &modified).unwrap();
    (base_chd, modified_image, modified)
}

#[test]
fn chdman_reads_our_diff_against_a_parent() {
    let Some(chdman) = chdman_binary() else {
        eprintln!("skipping: no chdman binary found (set $CHDMAN)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (base_chd, modified_image, modified) = parent_case(chdman.as_path(), dir.path());
    expect_ok(
        Path::new(env!("CARGO_BIN_EXE_chdrs")),
        &[
            "-o",
            &dir.path().to_string_lossy(),
            "-p",
            &base_chd.to_string_lossy(),
            &modified_image.to_string_lossy(),
        ],
    );
    let mod_chd = dir.path().join("modified.chd");
    expect_ok(
        chdman.as_path(),
        &[
            "verify",
            "-i",
            &mod_chd.to_string_lossy(),
            "-ip",
            &base_chd.to_string_lossy(),
        ],
    );
    let extracted = dir.path().join("extracted.bin");
    expect_ok(
        chdman.as_path(),
        &[
            "extracthd",
            "-i",
            &mod_chd.to_string_lossy(),
            "-ip",
            &base_chd.to_string_lossy(),
            "-o",
            &extracted.to_string_lossy(),
        ],
    );
    assert_eq!(std::fs::read(&extracted).unwrap(), modified);
}

#[test]
fn we_read_chdman_diff_against_a_parent() {
    let Some(chdman) = chdman_binary() else {
        eprintln!("skipping: no chdman binary found (set $CHDMAN)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (base_chd, modified_image, modified) = parent_case(chdman.as_path(), dir.path());
    let mod_chd = dir.path().join("mod2.chd");
    expect_ok(
        chdman.as_path(),
        &[
            "createhd",
            "-i",
            &modified_image.to_string_lossy(),
            "-o",
            &mod_chd.to_string_lossy(),
            "-op",
            &base_chd.to_string_lossy(),
            "-hs",
            "4096",
        ],
    );
    expect_ok(
        Path::new(env!("CARGO_BIN_EXE_chdrs")),
        &[
            "-o",
            &dir.path().to_string_lossy(),
            "-p",
            &base_chd.to_string_lossy(),
            &mod_chd.to_string_lossy(),
        ],
    );
    assert_eq!(
        std::fs::read(dir.path().join("mod2.iso")).unwrap(),
        modified,
        "chdrs extracted a different image from the parent-relative CHD chdman wrote"
    );
}

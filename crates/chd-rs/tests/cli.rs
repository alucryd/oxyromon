//! Agreement with the reference `chdman` implementation on the repository's
//! CHD fixtures: `info` prints the same report and `verify` reaches the same
//! verdict. The interop tests are skipped unless a `chdman` binary is
//! reachable through `$CHDMAN` or `$PATH`; the golden ones run always.
//!
//! The LaserDisc-flavored `(RDSK)` fixture is left out: the `avhu` codec is
//! not ported yet.

#![cfg(feature = "cli")]

use std::path::{Path, PathBuf};
use std::process::Command;

const FIXTURES: &[&str] = &[
    "Test Game (USA, Europe) (ISO).chd",
    "Test Game (USA, Europe) (Multiple Tracks).chd",
    "Test Game (USA, Europe) (Single Track).chd",
];

/// The report `chdrs info` gives for the DVD fixture, everything after the
/// `Input file:` line, which names the path as it was given.
const ISO_INFO: &str = "\
File Version: 5
Logical size: 358,400 bytes
Hunk Size:    4,096 bytes
Total Hunks:  88
Unit Size:    2,048 bytes
Total Units:  175
Compression:  lzma (LZMA), zlib (Deflate), huff (Huffman), flac (FLAC)
CHD size:     961 bytes
Ratio:        0.3%
SHA1:         7c6c71c660194f63551344dc28528b39dad11437
Data SHA1:    762a227d4d157c20e671b53041741ba6c22c552b
Metadata:     Tag='DVD '  Index=0  Length=1 bytes
              .
";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests")
        .join(name)
}

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

struct Run {
    success: bool,
    stdout: String,
    stderr: String,
}

fn run(bin: &str, args: &[&str]) -> Run {
    let out = Command::new(bin)
        .args(args)
        .output()
        .expect("failed to run chdrs");
    Run {
        success: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// What `chdman` reports, stripped of what `chdrs` leaves out: the leading
/// `chdman - MAME …` banner and, on a merged stream, the progress drawn
/// before the last carriage return.
fn report(output: &str) -> &str {
    let output = output.rsplit('\r').next().unwrap_or(output);
    output
        .strip_prefix("chdman - MAME")
        .and_then(|rest| rest.split_once('\n'))
        .map_or(output, |(_, rest)| rest)
}

#[test]
fn info_matches_the_golden_report() {
    let run = run(
        env!("CARGO_BIN_EXE_chdrs"),
        &["info", &fixture(FIXTURES[0]).to_string_lossy()],
    );
    assert!(run.success, "chdrs info failed: {}", run.stderr);
    assert_eq!(run.stdout.split_once('\n').unwrap().1, ISO_INFO);
}

#[test]
fn info_agrees_with_chdman() {
    let Some(chdman) = chdman_binary() else {
        eprintln!("skipping: no chdman binary found (set $CHDMAN)");
        return;
    };
    for name in FIXTURES {
        let input = fixture(name);
        let lossy = input.to_string_lossy().into_owned();
        let ours = run(env!("CARGO_BIN_EXE_chdrs"), &["info", &lossy]);
        let theirs = run(&chdman.to_string_lossy(), &["info", "-i", &lossy]);
        assert!(ours.success, "chdrs info failed: {}", ours.stderr);
        assert!(theirs.success, "chdman info failed: {}", theirs.stderr);
        // Drop the banner line and the input file line, which quote paths.
        let theirs: String = theirs
            .stdout
            .lines()
            .skip(2)
            .flat_map(|line| [line, "\n"])
            .collect();
        let ours: String = ours
            .stdout
            .lines()
            .skip(1)
            .flat_map(|line| [line, "\n"])
            .collect();
        assert_eq!(ours, theirs, "chdrs info disagrees for {name}");
    }
}

#[test]
fn verify_agrees_with_chdman() {
    let Some(chdman) = chdman_binary() else {
        eprintln!("skipping: no chdman binary found (set $CHDMAN)");
        return;
    };
    for name in FIXTURES {
        let input = fixture(name);
        let lossy = input.to_string_lossy().into_owned();
        let ours = run(env!("CARGO_BIN_EXE_chdrs"), &["verify", &lossy]);
        let theirs = run(&chdman.to_string_lossy(), &["verify", "-i", &lossy]);
        assert!(ours.success, "chdrs verify failed: {}", ours.stderr);
        assert!(theirs.success, "chdman verify failed: {}", theirs.stderr);
        assert_eq!(
            ours.stdout,
            report(&theirs.stdout),
            "chdrs verify disagrees for {name}"
        );
    }
}

#[test]
fn verify_reports_a_raw_sha1_mismatch() {
    // Corrupt the raw SHA1 in the header of a copy of the DVD fixture; the
    // data still hashes to itself, so verify must report the mismatch, and
    // unlike `chdman`, which exits successfully, fail.
    let input = fixture(FIXTURES[0]);
    let mut bytes = std::fs::read(&input).unwrap();
    bytes[64] ^= 0x01;
    let dir = tempfile::tempdir().unwrap();
    let broken = dir.path().join("broken.chd");
    std::fs::write(&broken, bytes).unwrap();
    let run = run(
        env!("CARGO_BIN_EXE_chdrs"),
        &["verify", &broken.to_string_lossy()],
    );
    assert!(!run.success, "chdrs verify accepted a corrupt header");
    // The flipped byte is the first of the header's raw SHA1, 0x76 → 0x77;
    // the report names the header value first and the computed one second.
    assert_eq!(
        run.stderr.lines().collect::<Vec<_>>(),
        [
            "Error: Raw SHA1 in header = 772a227d4d157c20e671b53041741ba6c22c552b",
            "              actual SHA1 = 762a227d4d157c20e671b53041741ba6c22c552b",
        ]
    );
}

//! Known answers: an IPS and a BPS Flips made from `source.bin` to `target.bin`,
//! the BPS with every action, and hand-built IPS for what Flips cannot create.

use std::path::{Path, PathBuf};
use tempfile::TempDir;
use xps_rs::{Error, Format, Warning, apply, identify};

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

/// A patch's warning and what it wrote.
type Applied = xps_rs::Result<(Option<Warning>, Vec<u8>)>;

fn run(source: &Path, patch: &Path) -> (TempDir, Applied) {
    let directory = TempDir::new().unwrap();
    let output = directory.path().join("out.bin");
    let result = apply(source, patch, &output, &mut |_| {})
        .map(|warning| (warning, std::fs::read(&output).unwrap()));
    (directory, result)
}

/// An IPS patch of `records` (offset, bytes) and `runs` (offset, count, byte).
fn ips(records: &[(u32, &[u8])], runs: &[(u32, u16, u8)], truncate: Option<u32>) -> Vec<u8> {
    let mut patch = b"PATCH".to_vec();
    for (offset, bytes) in records {
        patch.extend(&offset.to_be_bytes()[1..]);
        patch.extend((bytes.len() as u16).to_be_bytes());
        patch.extend(*bytes);
    }
    for (offset, count, byte) in runs {
        patch.extend(&offset.to_be_bytes()[1..]);
        patch.extend([0, 0]);
        patch.extend(count.to_be_bytes());
        patch.push(*byte);
    }
    patch.extend(b"EOF");
    if let Some(truncate) = truncate {
        patch.extend(&truncate.to_be_bytes()[1..]);
    }
    patch
}

#[test]
fn formats_are_told_by_their_magic() {
    assert_eq!(identify(&data("patch.ips")).unwrap(), Format::Ips);
    assert_eq!(identify(&data("patch.bps")).unwrap(), Format::Bps);
    assert!(matches!(
        identify(&data("source.bin")),
        Err(Error::BadMagic { .. })
    ));
}

#[test]
fn flips_patches_reproduce_their_target() {
    let target = std::fs::read(data("target.bin")).unwrap();
    for patch in ["patch.ips", "patch.bps"] {
        let (_directory, result) = run(&data("source.bin"), &data(patch));
        assert_eq!(result.unwrap(), (None, target.clone()), "{patch}");
    }
}

#[test]
fn progress_adds_up_to_the_patch_size() {
    for patch in ["patch.ips", "patch.bps"] {
        let directory = TempDir::new().unwrap();
        let mut total = 0;
        apply(
            &data("source.bin"),
            &data(patch),
            &directory.path().join("o"),
            &mut |n| total += n,
        )
        .unwrap();
        assert_eq!(
            total,
            std::fs::metadata(data(patch)).unwrap().len(),
            "{patch}"
        );
    }
}

#[test]
fn ips_runs_growth_and_truncation_follow_flips() {
    let directory = TempDir::new().unwrap();
    let source = directory.path().join("source.bin");
    std::fs::write(&source, vec![7u8; 100]).unwrap();
    let cases: [(Vec<u8>, Vec<u8>, Option<Warning>); 4] = [
        // A run, and a record past the end growing the file with zeroes between.
        (
            ips(&[(110, b"END")], &[(4, 3, 0xAA)], None),
            {
                let mut expected = vec![7u8; 100];
                expected[4..7].fill(0xAA);
                expected.resize(110, 0);
                expected.extend(b"END");
                expected
            },
            None,
        ),
        // Truncated to 50.
        (
            ips(&[(0, b"A")], &[], Some(50)),
            {
                let mut expected = vec![7u8; 50];
                expected[0] = b'A';
                expected
            },
            None,
        ),
        // A record past its own truncation: written, then cut.
        (
            ips(&[(90, b"PAST")], &[], Some(80)),
            vec![7u8; 80],
            Some(Warning::Scrambled),
        ),
        // Truncating to more than the source has.
        (
            ips(&[(0, b"B")], &[], Some(200)),
            {
                let mut expected = vec![7u8; 100];
                expected[0] = b'B';
                expected
            },
            Some(Warning::NotThis),
        ),
    ];
    for (i, (patch, expected, warning)) in cases.into_iter().enumerate() {
        let path = directory.path().join(format!("{i}.ips"));
        std::fs::write(&path, patch).unwrap();
        let (_out, result) = run(&source, &path);
        assert_eq!(result.unwrap(), (warning, expected), "case {i}");
    }
}

#[test]
fn an_ips_applied_to_its_output_says_so() {
    let (_directory, result) = run(&data("target.bin"), &data("patch.ips"));
    let (warning, output) = result.unwrap();
    assert_eq!(warning, Some(Warning::AlreadyApplied));
    assert_eq!(output, std::fs::read(data("target.bin")).unwrap());
}

#[test]
fn a_bps_refuses_the_wrong_source_and_leaves_nothing() {
    let directory = TempDir::new().unwrap();
    let output = directory.path().join("out.bin");
    std::fs::write(&output, "keep me").unwrap();
    let wrong = directory.path().join("wrong.bin");
    let mut bytes = std::fs::read(data("source.bin")).unwrap();
    bytes[10] ^= 1;
    std::fs::write(&wrong, bytes).unwrap();

    for (source, message) in [
        (wrong.as_path(), "checksum"),
        (data("target.bin").as_path(), "produces already"),
    ] {
        let result = apply(source, &data("patch.bps"), &output, &mut |_| {});
        assert!(
            matches!(&result, Err(Error::WrongSource(m)) if m.contains(message)),
            "{result:?}"
        );
    }
    assert_eq!(std::fs::read(&output).unwrap(), b"keep me");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
}

#[test]
fn malformed_patches_fail_without_panicking() {
    let directory = TempDir::new().unwrap();
    let output = directory.path().join("out.bin");
    for patch in ["patch.bps", "patch.ips"] {
        let bytes = std::fs::read(data(patch)).unwrap();
        let path = directory.path().join(patch);
        // Every truncation, and a flipped byte at every position; a sample of
        // them for the IPS, which runs to tens of thousands.
        let step = (bytes.len() / 500).max(1);
        for case in (0..bytes.len() * 2).step_by(step) {
            let mangled = if case < bytes.len() {
                bytes[..case].to_vec()
            } else {
                let mut mangled = bytes.clone();
                mangled[case - bytes.len()] ^= 0x5A;
                mangled
            };
            std::fs::write(&path, mangled).unwrap();
            let _ = apply(&data("source.bin"), &path, &output, &mut |_| {});
        }
    }
}

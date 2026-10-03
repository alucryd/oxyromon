//! Against the reference: patches the installed Flips creates now, applied by
//! both. Skipped when it is missing. Flips crashes creating linear BPS and
//! shrinking IPS, so those are covered by `apply.rs` instead.

use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn flips() -> bool {
    Command::new("flips").arg("--version").output().is_ok()
}

fn rom(len: usize, seed: &mut u64) -> Vec<u8> {
    let mut random = || {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    };
    let words: [&[u8]; 4] = [
        b"SPRITE",
        b"LEVEL",
        b"\0\0\0\0\0\0\0\0",
        b"\xff\xff\xff\xff",
    ];
    let mut bytes = Vec::with_capacity(len);
    while bytes.len() < len {
        if random() % 4 == 0 {
            bytes.extend((0..random() % 24).map(|_| random() as u8));
        } else {
            for _ in 0..random() % 8 {
                bytes.extend_from_slice(words[(random() % 4) as usize]);
            }
        }
    }
    bytes.truncate(len);
    bytes
}

#[test]
fn flips_and_xps_rs_agree() {
    if !flips() {
        eprintln!("flips not installed, skipping");
        return;
    }
    let directory = TempDir::new().unwrap();
    let path = |name: &str| directory.path().join(name);
    let mut seed = 0x9E37_79B9_7F4A_7C15;
    let source = rom(4 << 20, &mut seed);
    let mut edited = source.clone();
    for i in (0..edited.len()).step_by(70_000) {
        edited[i..i + 40].copy_from_slice(&rom(40, &mut seed));
    }
    let mut grown = edited.clone();
    grown.extend(rom(300_000, &mut seed));
    let mut moved = edited.clone();
    moved.splice(100_000..100_000, b"TRANSLATED ".repeat(4_000));
    moved.drain(2_000_000..2_300_000);
    moved.extend_from_slice(&source[5_000..900_000]);
    std::fs::write(path("source.bin"), &source).unwrap();

    for (name, target) in [("edited", edited), ("grown", grown), ("moved", moved)] {
        std::fs::write(path(name), &target).unwrap();
        for kind in ["--ips", "--bps-delta"] {
            let patch = path(&format!("{name}{kind}"));
            let created = Command::new("flips")
                .args(["--create", kind])
                .arg(path("source.bin"))
                .arg(path(name))
                .arg(&patch)
                .output()
                .unwrap();
            assert!(created.status.success(), "{name} {kind}");
            let output = path(&format!("{name}{kind}.out"));
            xps_rs::apply(&path("source.bin"), &patch, &output, &mut |_| {}).unwrap();
            assert!(std::fs::read(&output).unwrap() == target, "{name} {kind}");
        }
    }
}

#[test]
fn a_flips_ips_warning_is_ours_too() {
    if !flips() {
        return;
    }
    let directory = TempDir::new().unwrap();
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let reference = directory.path().join("reference.bin");
    let flips = Command::new("flips")
        .arg("--apply")
        .arg(data.join("patch.ips"))
        .arg(data.join("target.bin"))
        .arg(&reference)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&flips.stdout).contains("did nothing"));
    let output = directory.path().join("out.bin");
    let warning = xps_rs::apply(
        &data.join("target.bin"),
        &data.join("patch.ips"),
        &output,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(warning, Some(xps_rs::Warning::AlreadyApplied));
    assert!(std::fs::read(output).unwrap() == std::fs::read(reference).unwrap());
}

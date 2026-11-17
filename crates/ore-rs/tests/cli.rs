//! `orers` writes several inputs to a store in one run when given
//! `--parity`, a new store's or an existing one's, and checks a store given
//! as an input before them or after them.

// It runs orers
#![cfg(feature = "cli")]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;

/// A cartridge's ROM of 64 KiB, its bytes from `seed`, taken whole as a
/// SNES ROM.
fn rom(dir: &Path, name: &str, seed: u64) -> PathBuf {
    let mut state = seed;
    let bytes: Vec<u8> = (0..64 << 10)
        .map(|_| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (state >> 56) as u8
        })
        .collect();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// Run orers on `inputs`, writing to the store in `dir`, with `parity` if
/// given, and fail if it has not exited within a minute, as a run waiting
/// on a lock it holds itself never does.
fn orers(dir: &Path, parity: Option<&str>, inputs: &[&Path]) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_orers"));
    command.args(["--platform", "snes"]);
    if let Some(parity) = parity {
        command.args(["--parity", parity]);
    }
    let mut child = command
        .arg("--store")
        .arg(dir.join(".ore-store"))
        .arg("-o")
        .arg(dir)
        .args(inputs)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "orers {inputs:?}: {status}");
            return;
        }
        if start.elapsed() > Duration::from_secs(60) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("orers {inputs:?} still running after a minute");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// Each ROM comes back out of its thin ORE as it went in.
fn check(dir: &Path, roms: &[PathBuf]) {
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    for rom in roms {
        let ore = rom.with_extension("ore");
        let back = out.join(rom.file_name().unwrap());
        ore_rs::decompress(&ore, &back, &mut |_| {}).unwrap();
        assert_eq!(std::fs::read(&back).unwrap(), std::fs::read(rom).unwrap());
    }
}

#[test]
fn store_with_parity_takes_several_inputs() {
    let dir = TempDir::new().unwrap();
    let store = dir.path().join(".ore-store");
    let roms: Vec<PathBuf> = (1..=4)
        .map(|seed| rom(dir.path(), &format!("{seed}.sfc"), seed))
        .collect();

    // A new store, made with the parity asked for
    let inputs: Vec<&Path> = roms.iter().map(PathBuf::as_path).collect();
    orers(dir.path(), Some("8"), &inputs[..2]);
    check(dir.path(), &roms[..2]);
    assert!(!ore_rs::set_parity(&store, 8).unwrap());

    // An existing store, set to another parity
    orers(dir.path(), Some("0"), &inputs[2..]);
    check(dir.path(), &roms);
    assert!(!ore_rs::set_parity(&store, 0).unwrap());
}

#[test]
fn store_checked_before_the_inputs_is_set_up_for_them() {
    let dir = TempDir::new().unwrap();
    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let first = rom(&a, "1.sfc", 1);
    orers(&a, None, &[&first]);

    // Another store checked first opens no batch, so the first ROM after it
    // still makes the new store, with the parity asked for
    let roms = [rom(&b, "2.sfc", 2), rom(&b, "3.sfc", 3)];
    orers(&b, Some("4"), &[&a.join(".ore-store"), &roms[0], &roms[1]]);
    check(&b, &roms);
    assert!(!ore_rs::set_parity(&b.join(".ore-store"), 4).unwrap());
}

#[test]
fn store_checked_after_the_inputs_written_to_it() {
    let dir = TempDir::new().unwrap();
    let store = dir.path().join(".ore-store");
    let roms = [rom(dir.path(), "1.sfc", 1), rom(dir.path(), "2.sfc", 2)];

    // Checking it takes its lock, which the open batch holds until committed
    orers(dir.path(), None, &[&roms[0], &store]);
    check(dir.path(), &roms[..1]);

    // And setting its parity first, too
    orers(dir.path(), Some("2"), &[&roms[1], &store]);
    check(dir.path(), &roms);
    assert!(!ore_rs::set_parity(&store, 2).unwrap());
}

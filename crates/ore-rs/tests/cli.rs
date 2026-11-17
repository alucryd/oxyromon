//! `orers` writes several inputs to a store in one run when given
//! `--parity`, a new store's or an existing one's.

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

/// Write `roms` to the store in `dir` with `parity`, and fail if orers has
/// not exited within a minute, as a run waiting on a lock it holds itself
/// never does.
fn orers(dir: &Path, parity: &str, roms: &[PathBuf]) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_orers"))
        .args(["--platform", "snes", "--parity", parity, "--store"])
        .arg(dir.join(".ore-store"))
        .arg("-o")
        .arg(dir)
        .args(roms.iter().map(|rom| rom.as_os_str()))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "orers --parity {parity}: {status}");
            return;
        }
        if start.elapsed() > Duration::from_secs(60) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("orers --parity {parity} still running after a minute");
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
    orers(dir.path(), "8", &roms[..2]);
    check(dir.path(), &roms[..2]);
    assert!(!ore_rs::set_parity(&store, 8).unwrap());

    // An existing store, set to another parity
    orers(dir.path(), "0", &roms[2..]);
    check(dir.path(), &roms);
    assert!(!ore_rs::set_parity(&store, 0).unwrap());
}

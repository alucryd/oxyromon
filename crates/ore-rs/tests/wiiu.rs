#![cfg(feature = "write")]
//! A Wii U disc as an ORE, on the synthetic disc wux-rs is tested on: its
//! update partition's system and vWii titles decrypted under the disc key,
//! its game's hashed and unhashed contents, its free space repeats; read
//! back byte for byte, whole and at any offset.

#[path = "wiiu_common/mod.rs"]
mod wiiu_common;

use std::path::Path;

use ore_rs::{Error, Options, Reader};
use tempfile::TempDir;
use wiiu_common::{COMMON_KEY, DISC_KEY, VWII_KEY};

fn write(dir: &Path, keys: &[[u8; 16]]) -> (Vec<u8>, std::path::PathBuf) {
    let wud = wiiu_common::wud();
    let input = dir.join("Game.wud");
    std::fs::write(&input, &wud).unwrap();
    let ore = dir.join("Game.ore");
    ore_rs::compress(&input, &ore, &Options::default(), keys, &mut |_| {}).unwrap();
    (wud, ore)
}

/// Some 64-bit noise from `state`, for offsets.
fn next(state: &mut u64) -> u64 {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
    *state >> 11
}

#[test]
fn a_wii_u_disc_reads_back_as_it_was() {
    let dir = TempDir::new().unwrap();
    let (wud, ore) = write(dir.path(), &[COMMON_KEY, DISC_KEY, VWII_KEY]);
    let mut reader = Reader::open(&ore).unwrap();
    assert_eq!(reader.size(), wud.len() as u64);

    // Whole, then at offsets and lengths that cross every layer's blocks
    let mut back = vec![0; wud.len()];
    reader.read_at(0, &mut back).unwrap();
    assert!(back == wud);
    let mut state = 7;
    for _ in 0..300 {
        let len = (next(&mut state) % 200_000) as usize + 1;
        let at = next(&mut state) % (wud.len() - len) as u64;
        let mut buf = vec![0; len];
        reader.read_at(at, &mut buf).unwrap();
        assert!(
            buf[..] == wud[at as usize..at as usize + len],
            "{at:#x}+{len:#x}"
        );
    }

    // And decompressed, its hashes checked
    let out = dir.path().join("back.wud");
    ore_rs::decompress(&ore, &out, &mut |_| {}).unwrap();
    assert!(std::fs::read(&out).unwrap() == wud);
}

#[test]
fn a_wii_u_disc_needs_its_keys_to_be_written_only() {
    let dir = TempDir::new().unwrap();
    let wud = dir.path().join("Game.wud");
    std::fs::write(&wud, wiiu_common::wud()).unwrap();
    let ore = dir.path().join("Game.ore");
    // Without the disc key
    assert!(matches!(
        ore_rs::compress(&wud, &ore, &Options::default(), &[COMMON_KEY], &mut |_| {}),
        Err(Error::MissingKey(_))
    ));
    assert!(!ore.exists());
    // Without the vWii common key, its vWii title is kept encrypted, and
    // the disc reads back all the same, with no key at all
    let (bytes, ore) = write(dir.path(), &[COMMON_KEY, DISC_KEY]);
    let mut back = vec![0; bytes.len()];
    Reader::open(&ore).unwrap().read_at(0, &mut back).unwrap();
    assert!(back == bytes);
}

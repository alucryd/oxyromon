//! A game of several ROMs, a floppy's disks or a cartridge's chips, is one
//! ORE: written from the folder holding them, named its platform, each file
//! an area of its own and hashed on its own, and written back as that
//! folder, every file under its name.

// They write the OREs they read
#![cfg(feature = "write")]

use std::path::{Path, PathBuf};

use ore_rs::{Options, Reader};
use sha1::{Digest, Sha1};
use tempfile::TempDir;

/// A disk of `len` bytes, noise seeded by `seed`, its second half the
/// same on every disk, as a game's disks share their loader.
fn disk(dir: &Path, name: &str, seed: u64, len: usize) -> PathBuf {
    let mut state = seed;
    let mut bytes: Vec<u8> = (0..len / 2)
        .map(|_| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (state >> 56) as u8
        })
        .collect();
    let mut shared = 99u64;
    bytes.extend((0..len - len / 2).map(|_| {
        shared = shared.wrapping_mul(6364136223846793005).wrapping_add(1);
        (shared >> 56) as u8
    }));
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// A game of three disks in `dir`, a folder named after it.
fn game(dir: &Path) -> PathBuf {
    let folder = dir.join("Game (Europe)");
    std::fs::create_dir(&folder).unwrap();
    // Written out of order: the ORE takes them by name
    disk(&folder, "Game (Europe) (Disk 3).ipf", 3, 300_000);
    disk(&folder, "Game (Europe) (Disk 1).ipf", 1, 900_001);
    disk(&folder, "Game (Europe) (Disk 2).ipf", 2, 4096);
    folder
}

fn st() -> Options {
    Options {
        platform: Some("st"),
        ..Options::default()
    }
}

fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The ORE holds every disk, in order, and comes back as the folder.
fn assert_round_trips(folder: &Path, ore: &Path, dir: &Path) {
    let reader = Reader::open(ore).unwrap();
    assert!(reader.is_set());
    assert_eq!(reader.platform(), "st");
    // No one name: each disk has its own
    assert_eq!(reader.file_name(), None);
    let files = reader.files().unwrap();
    assert_eq!(
        files
            .iter()
            .map(|(name, _, _)| name.clone())
            .collect::<Vec<_>>(),
        names(folder)
    );
    let mut start = 0;
    for (name, at, len) in &files {
        let bytes = std::fs::read(folder.join(name)).unwrap();
        assert_eq!((*at, *len), (start, bytes.len() as u64), "{name}");
        // Each read where the byte view holds it
        let mut read = vec![0; bytes.len()];
        Reader::open(ore).unwrap().read_at(*at, &mut read).unwrap();
        assert_eq!(read, bytes, "{name}");
        // And hashed on its own, as a DAT lists it
        let sha1 = reader
            .identity()
            .find(|(view, algo, _)| *algo == "sha-1" && view == name)
            .map(|(_, _, value)| value.to_vec());
        assert_eq!(sha1, Some(Sha1::digest(&bytes).to_vec()), "{name}");
        start += len;
    }
    assert_eq!(reader.size(), start);

    let output = dir.join("out");
    ore_rs::decompress(ore, &output, &mut |_| {}).unwrap();
    assert_eq!(names(&output), names(folder));
    for name in names(folder) {
        assert_eq!(
            std::fs::read(output.join(&name)).unwrap(),
            std::fs::read(folder.join(&name)).unwrap(),
            "{name}"
        );
    }
}

#[test]
fn a_game_of_several_disks_is_one_ore_and_comes_back_as_its_folder() {
    let dir = TempDir::new().unwrap();
    let folder = game(dir.path());
    let ore = dir.path().join("Game (Europe).ore");
    ore_rs::compress(&folder, &ore, &st(), &[], &mut |_| {}).unwrap();
    assert_round_trips(&folder, &ore, dir.path());
}

#[test]
fn a_game_of_several_disks_goes_into_a_store_as_one_thin_ore() {
    let dir = TempDir::new().unwrap();
    let folder = game(dir.path());
    let games = dir.path().join("games");
    let store = games.join(".ore-store");
    ore_rs::create_store(&store, &Default::default()).unwrap();
    let ore = games.join("Game (Europe).ore");
    ore_rs::compress_to_store(&folder, &ore, &store, &st(), &[], &mut |_| {}).unwrap();
    assert!(Reader::inspect(&ore).unwrap().store_uuid().is_some());
    assert_round_trips(&folder, &ore, dir.path());
}

#[test]
fn a_game_of_one_rom_in_a_folder_is_that_rom() {
    let dir = TempDir::new().unwrap();
    let folder = dir.path().join("Game (Europe)");
    std::fs::create_dir(&folder).unwrap();
    let rom = disk(&folder, "Game (Europe).ipf", 1, 70_000);
    let ore = dir.path().join("Game (Europe).ore");
    ore_rs::compress(&folder, &ore, &st(), &[], &mut |_| {}).unwrap();
    let reader = Reader::open(&ore).unwrap();
    assert!(!reader.is_set());
    assert_eq!(reader.file_name(), Some("Game (Europe).ipf"));
    let output = dir.path().join("Game (Europe).ipf");
    ore_rs::decompress(&ore, &output, &mut |_| {}).unwrap();
    assert_eq!(std::fs::read(output).unwrap(), std::fs::read(rom).unwrap());
}

#[test]
fn a_folder_within_a_set_is_refused_and_leaves_nothing() {
    let dir = TempDir::new().unwrap();
    let folder = game(dir.path());
    std::fs::create_dir(folder.join("extras")).unwrap();
    let ore = dir.path().join("Game (Europe).ore");
    let error = ore_rs::compress(&folder, &ore, &st(), &[], &mut |_| {}).unwrap_err();
    assert!(matches!(error, ore_rs::Error::Unsupported(_)), "{error}");
    assert!(!ore.exists());
}

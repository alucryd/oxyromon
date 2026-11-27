#![cfg(feature = "write")]
//! A Wii U disc as an ORE, on the synthetic disc wux-rs is tested on: its
//! update partition's system and vWii titles decrypted under the disc key,
//! its game's hashed and unhashed contents, its free space repeats; read
//! back byte for byte, whole and at any offset.

use test_support::wiiu as wiiu_common;

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
fn a_long_repeat_reads_on_a_small_stack() {
    // Free space a thousand sectors long, repeating a region a sector long,
    // as a disc's gigabytes of it do: a read that followed the repeat back
    // a period at a time took a stack frame per period, and overflowed a
    // tokio worker's 2 MiB on a real disc. Read on a 64 KiB stack here, a
    // normal read's needs twice over and a frame per period's far under.
    let dir = TempDir::new().unwrap();
    let (wud, _) = wiiu_common::wud_with_free(&DISC_KEY, 1024);
    let input = dir.path().join("Game.wud");
    std::fs::write(&input, &wud).unwrap();
    let ore = dir.path().join("Game.ore");
    ore_rs::compress(
        &input,
        &ore,
        &Options::default(),
        &[COMMON_KEY, DISC_KEY, VWII_KEY],
        &mut |_| {},
    )
    .unwrap();
    let back = std::thread::Builder::new()
        .stack_size(64 << 10)
        .spawn(move || {
            let mut reader = Reader::open(&ore).unwrap();
            let mut back = vec![0; reader.size() as usize];
            reader.read_at(0, &mut back).unwrap();
            back
        })
        .unwrap()
        .join()
        .unwrap();
    assert!(back == wud);
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

/// Every file in `dir`, with its contents, sorted by name.
fn files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read(&path).unwrap())
        })
        .collect();
    files.sort();
    files
}

#[test]
fn an_eshop_title_comes_back_as_its_folder() {
    let dir = TempDir::new().unwrap();
    let nus = dir.path().join("Update");
    std::fs::create_dir(&nus).unwrap();
    wiiu_common::nus(&nus);
    assert!(ore_rs::is_title_folder(&nus));
    let ore = dir.path().join("Update.ore");
    ore_rs::compress(&nus, &ore, &Options::default(), &[COMMON_KEY], &mut |_| {}).unwrap();

    let reader = Reader::open(&ore).unwrap();
    assert!(reader.is_title());
    // Each file's hashes, the ticket's among them, as a DAT would list them
    for (name, _) in files(&nus) {
        assert!(
            reader
                .identity()
                .any(|(view, algo, _)| view == name && algo == "sha-1")
        );
    }
    let back = dir.path().join("back");
    ore_rs::decompress(&ore, &back, &mut |_| {}).unwrap();
    assert!(files(&back) == files(&nus));
}

#[test]
fn an_eshop_title_reads_file_by_file_from_its_byte_view() {
    let dir = TempDir::new().unwrap();
    let nus = dir.path().join("Update");
    std::fs::create_dir(&nus).unwrap();
    wiiu_common::nus(&nus);
    let ore = dir.path().join("Update.ore");
    ore_rs::compress(&nus, &ore, &Options::default(), &[COMMON_KEY], &mut |_| {}).unwrap();

    let mut reader = Reader::open(&ore).unwrap();
    let mut read: Vec<(String, Vec<u8>)> = Vec::new();
    for (name, start, len) in reader.files().unwrap() {
        let mut bytes = vec![0; len as usize];
        reader.read_at(start, &mut bytes).unwrap();
        read.push((name, bytes));
    }
    let (name, ticket) = reader.ticket().unwrap();
    read.push((name.to_owned(), ticket.to_vec()));
    read.sort();
    assert!(read == files(&nus));

    // A disc is no title: no files, no ticket
    let (_, disc) = write(dir.path(), &[COMMON_KEY, DISC_KEY, VWII_KEY]);
    let disc = Reader::open(&disc).unwrap();
    assert!(disc.files().unwrap().is_empty());
    assert!(disc.ticket().is_none());
}

#[test]
fn an_eshop_title_without_its_ticket_is_kept_as_it_is() {
    let dir = TempDir::new().unwrap();
    let nus = dir.path().join("DLC");
    std::fs::create_dir(&nus).unwrap();
    wiiu_common::nus(&nus);
    std::fs::remove_file(nus.join("title.tik")).unwrap();
    let ore = dir.path().join("DLC.ore");
    // No key is needed, as nothing is decrypted
    ore_rs::compress(&nus, &ore, &Options::default(), &[], &mut |_| {}).unwrap();
    let back = dir.path().join("back");
    ore_rs::decompress(&ore, &back, &mut |_| {}).unwrap();
    assert!(files(&back) == files(&nus));
}

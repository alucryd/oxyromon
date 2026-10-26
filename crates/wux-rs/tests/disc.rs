//! WUA back to the disc it came from, byte for byte, on synthetic discs: the
//! update partition kept once in a store, decrypted, or in the WUA without
//! one, and sectors the layout gets wrong kept whole.

mod common;

use std::path::{Path, PathBuf};

use common::{
    COMMON_KEY, DISC_KEY, DISC_KEY_2, SECTOR, SYSTEM_PLAINTEXT, VWII_KEY, VWII_PLAINTEXT,
};
use sha1::{Digest, Sha1};
use wux_rs::{Error, Key};

fn keys() -> Vec<Key> {
    vec![COMMON_KEY, DISC_KEY, DISC_KEY_2, VWII_KEY]
}

/// A disc's WUD and WUX in a new folder.
fn disc(dir: &Path, name: &str, bytes: &[u8]) -> (PathBuf, PathBuf) {
    let (wud, wux) = (
        dir.join(format!("{name}.wud")),
        dir.join(format!("{name}.wux")),
    );
    std::fs::write(&wud, bytes).unwrap();
    wux_rs::compress(&wud, &wux, &mut |_| {}).unwrap();
    (wud, wux)
}

/// A file of what rebuilds a WUA's disc.
fn disc_file(wua: &Path, name: &str) -> Option<Vec<u8>> {
    common::archive_files(wua)
        .into_iter()
        .find(|(path, _)| path == &format!(".wud/{name}"))
        .map(|(_, bytes)| bytes)
}

/// The store's entries.
fn entries(store: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(store)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    entries
}

fn contains(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count()
}

#[test]
fn a_wua_rebuilds_its_disc_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let original = common::wud();
    let (_, wux) = disc(dir.path(), "g", &original);
    let (wua, store) = (dir.path().join("g.wua"), dir.path().join(wux_rs::STORE_DIR));
    wux_rs::to_wua(&[&wux], &wua, &keys(), Some(&store), &mut |_| {}).unwrap();

    // the layout explains every sector, and the update is in the store
    assert_eq!(disc_file(&wua, "patches").unwrap(), Vec::<u8>::new());
    assert_eq!(entries(&store).len(), 1);
    assert_eq!(
        wux_rs::store_entries(&wua).unwrap(),
        vec![entries(&store)[0].file_name().unwrap().to_str().unwrap()]
    );
    let info = wux_rs::disc_info(&wua).unwrap().unwrap();
    assert_eq!(info.size, original.len() as u64);
    assert_eq!(info.sha1, <[u8; 20]>::from(Sha1::digest(&original)));
    assert_eq!(info.crc32, crc32fast::hash(&original));

    // when
    let (wud_back, wux_back) = (dir.path().join("back.wud"), dir.path().join("back.wux"));
    let mut read = 0;
    wux_rs::wua_to_wud(&wua, &wud_back, Some(&store), &keys(), &mut |n| read += n).unwrap();
    wux_rs::wua_to_wux(&wua, &wux_back, Some(&store), &keys(), &mut |_| {}).unwrap();

    // then
    assert!(std::fs::read(&wud_back).unwrap() == original);
    assert!(std::fs::read(&wux_back).unwrap() == std::fs::read(&wux).unwrap());
    assert_eq!(read, std::fs::metadata(&wua).unwrap().len());
}

#[test]
fn discs_with_the_same_update_store_it_once_decrypted() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join(wux_rs::STORE_DIR);
    // The same update, encrypted with two disc keys.
    let discs = [common::wud(), common::wud_with(&DISC_KEY_2)];
    let mut wuas = Vec::new();
    for (i, bytes) in discs.iter().enumerate() {
        let (wud, _) = disc(dir.path(), &format!("d{i}"), bytes);
        let wua = dir.path().join(format!("d{i}.wua"));
        wux_rs::to_wua(&[&wud], &wua, &keys(), Some(&store), &mut |_| {}).unwrap();
        wuas.push(wua);
    }

    // one entry, whose system title is in it decrypted: its raw content
    // and its hashed one's data both start with the plaintext
    let entries = entries(&store);
    assert_eq!(entries.len(), 1);
    let entry = zstd::decode_all(std::fs::File::open(&entries[0]).unwrap()).unwrap();
    assert_eq!(contains(&entry, SYSTEM_PLAINTEXT), 2);
    // and its vWii title, with the vWii common key
    assert_eq!(contains(&entry, VWII_PLAINTEXT), 1);

    for (wua, bytes) in wuas.iter().zip(&discs) {
        let back = dir.path().join("back.wud");
        wux_rs::wua_to_wud(wua, &back, Some(&store), &keys(), &mut |_| {}).unwrap();
        assert!(&std::fs::read(&back).unwrap() == bytes);
    }
}

#[test]
fn without_a_store_the_update_stays_in_the_wua() {
    let dir = tempfile::tempdir().unwrap();
    let original = common::wud();
    let (wud, _) = disc(dir.path(), "g", &original);
    let wua = dir.path().join("g.wua");
    wux_rs::to_wua(&[&wud], &wua, &keys(), None, &mut |_| {}).unwrap();
    assert!(!dir.path().join(wux_rs::STORE_DIR).exists());
    // the update partition, second on the disc
    assert!(contains(&disc_file(&wua, "01/stream").unwrap(), SYSTEM_PLAINTEXT) == 2);

    let back = dir.path().join("back.wud");
    wux_rs::wua_to_wud(&wua, &back, None, &keys(), &mut |_| {}).unwrap();
    assert!(std::fs::read(&back).unwrap() == original);
}

#[test]
fn a_wua_needs_its_update_partition() {
    let dir = tempfile::tempdir().unwrap();
    let (wud, _) = disc(dir.path(), "g", &common::wud());
    let (wua, store) = (dir.path().join("g.wua"), dir.path().join("store"));
    wux_rs::to_wua(&[&wud], &wua, &keys(), Some(&store), &mut |_| {}).unwrap();
    let back = dir.path().join("back.wux");

    let elsewhere = dir.path().join("empty");
    std::fs::create_dir(&elsewhere).unwrap();
    for store in [None, Some(elsewhere.as_path())] {
        let result = wux_rs::wua_to_wux(&wua, &back, store, &keys(), &mut |_| {});
        assert!(
            matches!(&result, Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound),
            "{result:?}"
        );
        assert!(!back.exists());
    }

    // a damaged entry does not rebuild either
    let entry = &entries(&store)[0];
    let mut bytes = zstd::decode_all(std::fs::File::open(entry).unwrap()).unwrap();
    let at = bytes.len() - 0x100;
    bytes[at] ^= 1;
    std::fs::write(entry, zstd::encode_all(&bytes[..], 3).unwrap()).unwrap();
    let result = wux_rs::wua_to_wux(&wua, &back, Some(&store), &keys(), &mut |_| {});
    assert!(matches!(result, Err(Error::Corrupt(_))), "{result:?}");
    assert!(!back.exists());
    assert!(!dir.path().join("back.wux.part").exists());
}

#[test]
fn sectors_the_layout_gets_wrong_are_kept_whole() {
    let dir = tempfile::tempdir().unwrap();
    // The last sector, which should repeat the small game's last content.
    let mut original = common::wud();
    let last = original.len() - SECTOR;
    original[last + 7] ^= 0xFF;
    let (wud, _) = disc(dir.path(), "g", &original);
    let wua = dir.path().join("g.wua");
    wux_rs::to_wua(&[&wud], &wua, &keys(), None, &mut |_| {}).unwrap();
    assert_eq!(
        disc_file(&wua, "patches").unwrap(),
        original[last..].to_vec()
    );

    let back = dir.path().join("back.wud");
    wux_rs::wua_to_wud(&wua, &back, None, &keys(), &mut |_| {}).unwrap();
    assert!(std::fs::read(&back).unwrap() == original);
}

#[test]
fn a_wua_without_its_disc_does_not_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let update = dir.path().join("update");
    std::fs::create_dir(&update).unwrap();
    common::nus(&update);
    let wua = dir.path().join("u.wua");
    wux_rs::to_wua(&[&update], &wua, &keys(), None, &mut |_| {}).unwrap();
    assert_eq!(wux_rs::disc_info(&wua).unwrap(), None);

    let back = dir.path().join("back.wud");
    let result = wux_rs::wua_to_wud(&wua, &back, None, &keys(), &mut |_| {});
    assert!(matches!(result, Err(Error::Unsupported(_))), "{result:?}");
    assert!(!back.exists());
}

#[test]
fn a_vwii_title_needs_the_vwii_common_key_to_decrypt() {
    let dir = tempfile::tempdir().unwrap();
    let original = common::wud();
    let (wud, _) = disc(dir.path(), "g", &original);
    let without: Vec<Key> = vec![COMMON_KEY, DISC_KEY];

    // without it, the vWii title is kept as it is, and the disc still
    // rebuilds byte for byte
    let wua = dir.path().join("without.wua");
    wux_rs::to_wua(&[&wud], &wua, &without, None, &mut |_| {}).unwrap();
    let stream = disc_file(&wua, "01/stream").unwrap();
    assert_eq!(contains(&stream, SYSTEM_PLAINTEXT), 2);
    assert_eq!(contains(&stream, VWII_PLAINTEXT), 0);
    let back = dir.path().join("back.wud");
    wux_rs::wua_to_wud(&wua, &back, None, &without, &mut |_| {}).unwrap();
    assert!(std::fs::read(&back).unwrap() == original);

    // with it, it is decrypted, and rebuilding needs it again
    let wua = dir.path().join("with.wua");
    wux_rs::to_wua(&[&wud], &wua, &keys(), None, &mut |_| {}).unwrap();
    assert_eq!(
        contains(&disc_file(&wua, "01/stream").unwrap(), VWII_PLAINTEXT),
        1
    );
    let result = wux_rs::wua_to_wud(&wua, &back, None, &without, &mut |_| {});
    assert!(matches!(result, Err(Error::MissingKey(_))), "{result:?}");
    wux_rs::wua_to_wud(&wua, &back, None, &keys(), &mut |_| {}).unwrap();
    assert!(std::fs::read(&back).unwrap() == original);
}

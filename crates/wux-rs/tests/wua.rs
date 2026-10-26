//! WUD and WUX to WUA, on a synthetic disc, read back with an independent
//! ZArchive reader.

mod common;

use std::path::Path;

use common::{COMMON_KEY, DISC_KEY, FOLDER};
use wux_rs::{Error, Key};

/// Every file of the archive at `path` but the revert data and what
/// rebuilds the disc, which are checked on their own.
fn files(path: &Path) -> Vec<(String, Vec<u8>)> {
    common::archive_files(path)
        .into_iter()
        .filter(|(path, _)| !path.starts_with(".nus/") && !path.starts_with(".wud/"))
        .collect()
}

fn setup() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let (wud, wux) = (dir.path().join("g.wud"), dir.path().join("g.wux"));
    std::fs::write(&wud, common::wud()).unwrap();
    wux_rs::compress(&wud, &wux, &mut |_| {}).unwrap();
    (dir, wud, wux)
}

/// Keys as a keys.txt might list them: others first, the common key among
/// them, and the disc key last.
fn keys() -> Vec<Key> {
    vec![[0x99; 16], COMMON_KEY, [0x77; 16], DISC_KEY]
}

#[test]
fn a_disc_converts_to_the_files_of_its_game() {
    let (dir, wud, wux) = setup();
    let mut expected = common::game_files().1;
    expected.sort();
    for input in [&wud, &wux] {
        let output = dir.path().join("g.wua");
        let mut read = 0;
        let stats = wux_rs::to_wua(&[input], &output, &keys(), None, &mut |n| read += n).unwrap();
        assert_eq!(read, std::fs::metadata(input).unwrap().len());
        assert_eq!(stats.output_size, std::fs::metadata(&output).unwrap().len());
        let files = files(&output);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        let expected_names: Vec<&str> = expected.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, expected_names);
        assert_eq!(files, expected);
        assert!(files.iter().all(|(n, _)| n.starts_with(FOLDER)));
    }
}

#[test]
fn a_disc_without_its_keys_does_not_convert() {
    let (dir, wud, _) = setup();
    let output = dir.path().join("g.wua");
    let no_disc_key = wux_rs::to_wua(&[&wud], &output, &[COMMON_KEY], None, &mut |_| {});
    assert!(
        matches!(no_disc_key, Err(Error::MissingKey(_))),
        "{no_disc_key:?}"
    );
    let no_common_key = wux_rs::to_wua(&[&wud], &output, &[DISC_KEY], None, &mut |_| {});
    assert!(
        matches!(no_common_key, Err(Error::MissingKey(_))),
        "{no_common_key:?}"
    );
    assert!(!output.exists());
    assert!(!dir.path().join("g.wua.part").exists());
}

#[test]
fn a_damaged_hashed_block_fails_its_hash_check() {
    let (dir, wud, _) = setup();
    // A byte of data in the hashed cluster's first block.
    let (mut disc, hashed) = common::wud_and_hashed(&DISC_KEY);
    disc[hashed + 0x500] ^= 1;
    std::fs::write(&wud, disc).unwrap();
    let output = dir.path().join("g.wua");
    let result = wux_rs::to_wua(&[&wud], &output, &keys(), None, &mut |_| {});
    assert!(matches!(result, Err(Error::Corrupt(_))), "{result:?}");
    assert!(!output.exists());
}

#[test]
fn a_file_that_is_not_a_disc_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("x.wud");
    std::fs::write(&input, vec![0; 4 * common::SECTOR]).unwrap();
    let result = wux_rs::to_wua(
        &[&input],
        &dir.path().join("x.wua"),
        &keys(),
        None,
        &mut |_| {},
    );
    assert!(matches!(result, Err(Error::BadMagic { .. })), "{result:?}");
}

#[test]
fn a_game_and_its_update_make_one_wua() {
    let (dir, _, wux) = setup();
    let update = dir.path().join("update");
    std::fs::create_dir(&update).unwrap();
    let mut expected = common::game_files().1;
    expected.extend(common::nus(&update));
    expected.sort();
    let output = dir.path().join("g.wua");
    let mut read = 0;
    wux_rs::to_wua(&[&wux, &update], &output, &keys(), None, &mut |n| read += n).unwrap();
    let update_size: u64 = std::fs::read_dir(&update)
        .unwrap()
        .map(|e| e.unwrap().metadata().unwrap().len())
        .sum();
    assert_eq!(read, std::fs::metadata(&wux).unwrap().len() + update_size);
    assert_eq!(files(&output), expected);

    // An update alone makes a WUA of its own.
    let alone = dir.path().join("u.wua");
    wux_rs::to_wua(&[&update], &alone, &keys(), None, &mut |_| {}).unwrap();
    assert!(
        files(&alone)
            .iter()
            .all(|(n, _)| n.starts_with(common::UPDATE_FOLDER))
    );
}

#[test]
fn a_title_given_twice_is_refused() {
    let (dir, wud, wux) = setup();
    let output = dir.path().join("g.wua");
    let result = wux_rs::to_wua(&[&wud, &wux], &output, &keys(), None, &mut |_| {});
    assert!(matches!(result, Err(Error::Unsupported(_))), "{result:?}");
    assert!(!output.exists());
}

#[test]
fn an_update_named_as_no_intro_names_it_converts_the_same() {
    let (dir, wud, _) = setup();
    let update = dir.path().join("update");
    std::fs::create_dir(&update).unwrap();
    let mut expected = common::nus(&update);
    expected.sort();
    // The CDN's names: no `.app`, and the TMD and ticket by version.
    for entry in std::fs::read_dir(&update).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        let cdn = match name.as_str() {
            "title.tmd" => "tmd.48".to_string(),
            "title.tik" => "cetk.48".to_string(),
            app => app.trim_end_matches(".app").to_string(),
        };
        std::fs::rename(&path, update.join(cdn)).unwrap();
    }
    let output = dir.path().join("u.wua");
    wux_rs::to_wua(&[&update], &output, &keys(), None, &mut |_| {}).unwrap();
    assert_eq!(files(&output), expected);

    // A title's ID needs no key in a NUS folder, and the disc's keys on a disc.
    assert_eq!(
        wux_rs::title_id(&update, &[]).unwrap(),
        (common::UPDATE_ID, 48)
    );
    assert_eq!(
        wux_rs::title_id(&wud, &keys()).unwrap(),
        (common::TITLE_ID, 32)
    );
}

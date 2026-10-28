//! WUD and WUX to a NUS folder, on a synthetic disc.

mod common;

use std::path::Path;

use common::{COMMON_KEY, DISC_KEY};
use wux_rs::{Error, Key};

fn keys() -> Vec<Key> {
    vec![COMMON_KEY, DISC_KEY]
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn a_disc_converts_to_an_installable_folder() {
    let dir = tempfile::tempdir().unwrap();
    let (wud, wux) = (dir.path().join("g.wud"), dir.path().join("g.wux"));
    std::fs::write(&wud, common::wud()).unwrap();
    wux_rs::compress(&wud, &wux, &mut |_| {}).unwrap();
    let mut expected = common::game_files().1;
    expected.sort();
    for input in [&wud, &wux] {
        let output = dir.path().join("Test Game");
        let mut read = 0;
        wux_rs::to_nus(input, &output, &keys(), &mut |n| read += n).unwrap();
        assert_eq!(read, std::fs::metadata(input).unwrap().len());
        // The disc's TMD, ticket and certificates, and every content, the
        // hashed one with its H3 hashes.
        assert_eq!(
            names(&output),
            [
                "00000000.app",
                "00000001.app",
                "00000002.app",
                "00000002.h3",
                "title.cert",
                "title.tik",
                "title.tmd",
            ]
        );
        assert_eq!(
            std::fs::read(output.join("title.cert")).unwrap(),
            common::CERT
        );
        assert_eq!(
            std::fs::read(output.join("00000002.h3")).unwrap(),
            common::h3(
                std::fs::metadata(output.join("00000002.app"))
                    .unwrap()
                    .len() as usize
            )
        );
        // It is a title in its own right, with the disc's files.
        let wua = dir.path().join("g.wua");
        wux_rs::to_wua(&[&output], &wua, &keys(), None, &mut |_| {}).unwrap();
        let wua_of_disc = dir.path().join("d.wua");
        wux_rs::to_wua(&[input], &wua_of_disc, &keys(), None, &mut |_| {}).unwrap();
        // but for what rebuilds the disc, which only the disc's WUA holds
        let of_disc: Vec<_> = common::archive_files(&wua_of_disc)
            .into_iter()
            .filter(|(path, _)| !path.starts_with(".wud/"))
            .collect();
        assert!(common::archive_files(&wua) == of_disc);
        // Converting again replaces the folder.
        wux_rs::to_nus(input, &output, &keys(), &mut |_| {}).unwrap();
        assert!(!dir.path().join("Test Game.part").exists());
    }
}

#[test]
fn a_damaged_content_fails_its_tmd_hash() {
    let dir = tempfile::tempdir().unwrap();
    let wud = dir.path().join("g.wud");
    // The hashed cluster's first block holds the H2 hashes the H3 covers.
    let (mut disc, hashed) = common::wud_and_hashed(&common::DISC_KEY);
    disc[hashed + 0x300] ^= 1;
    std::fs::write(&wud, disc).unwrap();
    let output = dir.path().join("Test Game");
    let result = wux_rs::to_nus(&wud, &output, &keys(), &mut |_| {});
    assert!(matches!(result, Err(Error::Corrupt(_))), "{result:?}");
    assert!(!output.exists());
    assert!(!dir.path().join("Test Game.part").exists());
}

#[test]
fn a_folder_in_the_way_is_left_as_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let wud = dir.path().join("g.wud");
    std::fs::write(&wud, common::wud()).unwrap();
    // a folder of anything, and a NUS folder of another title, the update
    let other = dir.path().join("other");
    std::fs::create_dir(&other).unwrap();
    std::fs::write(other.join("save.dat"), b"mine").unwrap();
    let update = dir.path().join("update");
    std::fs::create_dir(&update).unwrap();
    common::nus(&update);
    for output in [other, update] {
        let before = names(&output);
        let result = wux_rs::to_nus(&wud, &output, &keys(), &mut |_| {});
        assert!(
            matches!(&result, Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists),
            "{result:?}"
        );
        assert_eq!(names(&output), before);
        let mut part = output.into_os_string();
        part.push(".part");
        assert!(!Path::new(&part).exists());
    }
}

#[test]
fn a_title_missing_an_h3_file_gets_it_back() {
    let dir = tempfile::tempdir().unwrap();
    common::nus(dir.path());
    let h3 = dir.path().join("00000012.h3");
    let expected = std::fs::read(&h3).unwrap();
    std::fs::remove_file(&h3).unwrap();
    let written = wux_rs::write_missing_h3(dir.path(), &keys()).unwrap();
    assert_eq!(written, vec![h3.clone()]);
    assert_eq!(std::fs::read(&h3).unwrap(), expected);
    assert!(!names(dir.path()).iter().any(|name| name.ends_with(".part")));
}

#[test]
fn a_title_with_its_h3_files_needs_no_key() {
    let dir = tempfile::tempdir().unwrap();
    common::nus(dir.path());
    // Named as downloaders name them, in upper case.
    std::fs::rename(
        dir.path().join("00000012.h3"),
        dir.path().join("00000012.H3"),
    )
    .unwrap();
    let before = names(dir.path());
    assert!(
        wux_rs::write_missing_h3(dir.path(), &[])
            .unwrap()
            .is_empty()
    );
    assert_eq!(names(dir.path()), before);
}

#[test]
fn a_missing_h3_file_needs_the_common_key() {
    let dir = tempfile::tempdir().unwrap();
    common::nus(dir.path());
    std::fs::remove_file(dir.path().join("00000012.h3")).unwrap();
    let error = wux_rs::write_missing_h3(dir.path(), &[DISC_KEY]).unwrap_err();
    assert!(matches!(error, Error::MissingKey(_)), "{error}");
    assert!(!dir.path().join("00000012.h3").exists());
}

#[test]
fn a_vwii_title_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    common::nus(dir.path());
    std::fs::remove_file(dir.path().join("00000012.h3")).unwrap();
    // A vWii title's ID, whose type is not the Wii U's 0005.
    let tmd_path = dir.path().join("title.tmd");
    let mut tmd = std::fs::read(&tmd_path).unwrap();
    tmd[0x18C..0x194].copy_from_slice(&0x0000_0007_4841_4341u64.to_be_bytes());
    std::fs::write(&tmd_path, tmd).unwrap();
    let before = names(dir.path());
    assert!(
        wux_rs::write_missing_h3(dir.path(), &[])
            .unwrap()
            .is_empty()
    );
    assert_eq!(names(dir.path()), before);
}

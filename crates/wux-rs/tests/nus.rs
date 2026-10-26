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

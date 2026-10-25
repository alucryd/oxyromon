//! WUA back to NUS folders, from the revert data a WUA keeps, on a synthetic
//! disc and its update.

mod common;

use std::path::Path;

use common::{COMMON_KEY, DISC_KEY, FOLDER, UPDATE_FOLDER};
use wux_rs::{Error, Key};

fn keys() -> Vec<Key> {
    vec![COMMON_KEY, DISC_KEY]
}

/// Every file in `dir`, with its contents.
fn files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let path = e.unwrap().path();
            (
                path.file_name().unwrap().to_str().unwrap().to_string(),
                std::fs::read(&path).unwrap(),
            )
        })
        .collect();
    files.sort();
    files
}

#[test]
fn a_wua_reverts_to_the_titles_it_was_made_from() {
    let dir = tempfile::tempdir().unwrap();
    let (wud, wux) = (dir.path().join("g.wud"), dir.path().join("g.wux"));
    std::fs::write(&wud, common::wud()).unwrap();
    wux_rs::compress(&wud, &wux, &mut |_| {}).unwrap();
    let update = dir.path().join("update");
    std::fs::create_dir(&update).unwrap();
    common::nus(&update);
    let wua = dir.path().join("g.wua");
    wux_rs::to_wua(&[&wux, &update], &wua, &keys(), &mut |_| {}).unwrap();

    // when
    let output = dir.path().join("reverted");
    let mut read = 0;
    wux_rs::wua_to_nus(&wua, &output, &keys(), &mut |n| read += n).unwrap();

    // then the disc's title is what converting the disc gives, byte for byte,
    // its hidden files and all, and the update what it came from
    assert_eq!(read, std::fs::metadata(&wua).unwrap().len());
    let disc_nus = dir.path().join("disc");
    wux_rs::to_nus(&wux, &disc_nus, &keys(), &mut |_| {}).unwrap();
    let names =
        |files: &[(String, Vec<u8>)]| files.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>();
    for (reverted, original) in [
        (output.join(FOLDER), disc_nus),
        (output.join(UPDATE_FOLDER), update),
    ] {
        let (reverted, original) = (files(&reverted), files(&original));
        assert_eq!(names(&reverted), names(&original));
        for ((name, a), (_, b)) in reverted.iter().zip(&original) {
            assert!(a == b, "{name} differs");
        }
    }
    assert!(!dir.path().join("reverted.part").exists());
}

#[test]
fn a_wua_without_revert_data_does_not_revert() {
    // A WUA as Cemu writes it: titles only.
    let dir = tempfile::tempdir().unwrap();
    let wua = dir.path().join("cemu.wua");
    let mut writer = zarust::ArchiveWriter::new(std::fs::File::create(&wua).unwrap());
    writer.make_dir(FOLDER, false).unwrap();
    writer
        .add_file(format!("{FOLDER}/meta.xml"), &b"<menu/>"[..])
        .unwrap();
    writer.finish().unwrap();
    let output = dir.path().join("reverted");
    let result = wux_rs::wua_to_nus(&wua, &output, &keys(), &mut |_| {});
    assert!(matches!(result, Err(Error::Unsupported(_))), "{result:?}");
    assert!(!output.exists());
}

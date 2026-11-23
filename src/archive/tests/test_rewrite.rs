use super::*;
use sevenz_rust2::ArchiveReader;
use std::io::Cursor;
use tempfile::TempDir;

/// Distinct, compressible contents, so a mix-up between entries shows.
fn content(name: &str) -> Vec<u8> {
    name.repeat(4096).into_bytes()
}

/// A 7z holding one solid block of `a`, `b` and `c`, then a block for `d` alone.
fn fixture(directory: &Path) -> PathBuf {
    let path = directory.join("game.7z");
    let mut writer = ArchiveWriter::create(&path).unwrap();
    let solid = ["a", "b", "c"];
    writer
        .push_archive_entries(
            solid
                .iter()
                .map(|name| ArchiveEntry::new_file(name))
                .collect(),
            solid
                .iter()
                .map(|name| Cursor::new(content(name)).into())
                .collect(),
        )
        .unwrap();
    writer
        .push_archive_entry(ArchiveEntry::new_file("d"), Some(Cursor::new(content("d"))))
        .unwrap();
    writer.finish().unwrap();
    path
}

/// Every entry as `(name, contents)`, in order.
fn entries(path: &Path) -> Vec<(String, Vec<u8>)> {
    let mut reader = ArchiveReader::open(path, Password::empty()).unwrap();
    let mut entries = Vec::new();
    reader
        .for_each_entries(|entry, data| {
            let mut buffer = Vec::new();
            data.read_to_end(&mut buffer)?;
            entries.push((entry.name.clone(), buffer));
            Ok(true)
        })
        .unwrap();
    entries
}

fn named(names: &[&str]) -> Vec<(String, Vec<u8>)> {
    names
        .iter()
        .map(|name| (name.to_string(), content(&name[name.len() - 1..])))
        .collect()
}

#[tokio::test]
async fn a_rename_copies_every_block_as_it_is() {
    let directory = TempDir::new().unwrap();
    let path = fixture(directory.path());
    let before = open_sevenzip(&path).unwrap().pack_sizes().to_vec();

    rename(&path, "b", "renamed/b").await.unwrap();

    assert_eq!(entries(&path), named(&["a", "renamed/b", "c", "d"]));
    assert_eq!(open_sevenzip(&path).unwrap().pack_sizes(), before);
}

#[tokio::test]
async fn a_delete_from_a_solid_block_encodes_what_it_leaves() {
    let directory = TempDir::new().unwrap();
    let path = fixture(directory.path());

    delete(&path, "b").await.unwrap();

    assert_eq!(entries(&path), named(&["a", "c", "d"]));
    let archive = open_sevenzip(&path).unwrap();
    // `d`'s block went across untouched.
    assert_eq!(archive.blocks.len(), 2);
    // Nothing is left behind next to the archive.
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn a_delete_of_a_whole_block_drops_just_it() {
    let directory = TempDir::new().unwrap();
    let path = fixture(directory.path());
    let solid = open_sevenzip(&path).unwrap().pack_sizes()[0];

    delete(&path, "d").await.unwrap();

    assert_eq!(entries(&path), named(&["a", "b", "c"]));
    assert_eq!(open_sevenzip(&path).unwrap().pack_sizes(), [solid]);
}

#[tokio::test]
async fn deleting_the_last_entry_removes_the_archive() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("single.7z");
    std::fs::write(directory.path().join("e"), content("e")).unwrap();
    create(
        &path,
        directory.path(),
        Path::new("e"),
        &ArchiveType::Sevenzip,
        &ArchiveCompression::Default,
        false,
    )
    .await
    .unwrap();

    delete(&path, "e").await.unwrap();

    assert!(!path.exists());
}

#[tokio::test]
async fn an_append_keeps_what_was_there() {
    let directory = TempDir::new().unwrap();
    let path = fixture(directory.path());
    let before = open_sevenzip(&path).unwrap().pack_sizes().to_vec();
    std::fs::write(directory.path().join("e"), content("e")).unwrap();

    create(
        &path,
        directory.path(),
        Path::new("e"),
        &ArchiveType::Sevenzip,
        &ArchiveCompression::Lzma2(1),
        false,
    )
    .await
    .unwrap();

    assert_eq!(entries(&path), named(&["a", "b", "c", "d", "e"]));
    assert_eq!(open_sevenzip(&path).unwrap().pack_sizes()[..2], before);
}

#[tokio::test]
async fn an_entry_is_extracted_from_the_middle_of_a_solid_block() {
    let directory = TempDir::new().unwrap();
    let path = fixture(directory.path());
    let out = TempDir::new().unwrap();

    extract(&path, "b", out.path()).await.unwrap();

    assert_eq!(std::fs::read(out.path().join("b")).unwrap(), content("b"));
}

//! Checks on a real disc, which can't be shipped: set `WUXRS_DISC` to a WUD
//! or WUX, and `WUXRS_KEYS` to a keys.txt holding its disc key and the Wii U
//! common key (Cemu's by default). Set `WUXRS_WUA` to a WUA Cemu made of the
//! same game to compare the game's files with it, and `WUXRS_ADD` to the NUS
//! folder of its update to convert that too. It also converts the disc to a
//! NUS folder, checking every content against its TMD, and reverts its WUA to
//! NUS folders, comparing them with the disc's and the update's.
//!
//! ```sh
//! WUXRS_DISC=game.wux WUXRS_WUA=game.wua cargo test --release -p wux-rs -- --ignored
//! ```

use std::io::Read;
use std::path::PathBuf;

use sha1::{Digest, Sha1};
use zarust::ArchiveReader;

fn disc() -> Option<PathBuf> {
    let path = std::env::var_os("WUXRS_DISC").map(PathBuf::from);
    if path.is_none() {
        eprintln!("WUXRS_DISC is unset, skipping");
    }
    path
}

fn sha1(mut reader: impl Read) -> [u8; 20] {
    let mut hasher = Sha1::new();
    let mut buf = vec![0; 1 << 20];
    loop {
        match reader.read(&mut buf).unwrap() {
            0 => break hasher.finalize().into(),
            n => hasher.update(&buf[..n]),
        }
    }
}

#[test]
#[ignore]
fn a_real_disc_survives_wux_and_back() {
    let Some(disc) = disc() else { return };
    let dir = tempfile::tempdir_in(disc.parent().unwrap()).unwrap();
    let original = sha1(wux_rs::Image::open(&disc).unwrap());
    let is_wux = wux_rs::Image::open(&disc).unwrap().header().is_some();
    let (wud, wux) = if is_wux {
        let wud = dir.path().join("disc.wud");
        wux_rs::decompress(&disc, &wud, &mut |_| {}).unwrap();
        let wux = dir.path().join("disc.wux");
        wux_rs::compress(&wud, &wux, &mut |_| {}).unwrap();
        (wud, wux)
    } else {
        let wux = dir.path().join("disc.wux");
        wux_rs::compress(&disc, &wux, &mut |_| {}).unwrap();
        let wud = dir.path().join("disc.wud");
        wux_rs::decompress(&wux, &wud, &mut |_| {}).unwrap();
        (wud, wux)
    };
    assert_eq!(sha1(std::fs::File::open(&wud).unwrap()), original);
    assert_eq!(sha1(wux_rs::Image::open(&wux).unwrap()), original);
}

#[test]
#[ignore]
fn a_real_disc_converts_to_what_cemu_makes() {
    let Some(disc) = disc() else { return };
    let keys = std::env::var_os("WUXRS_KEYS")
        .map(PathBuf::from)
        .or_else(wux_rs::cemu_keys_path)
        .unwrap();
    let keys = wux_rs::load_keys(&keys).unwrap();
    let dir = tempfile::tempdir_in(disc.parent().unwrap()).unwrap();
    let output = dir.path().join("disc.wua");
    let add = std::env::var_os("WUXRS_ADD").map(PathBuf::from);
    let mut inputs = vec![disc.as_path()];
    inputs.extend(add.as_deref());
    wux_rs::to_wua(&inputs, &output, &keys, &mut |_| {}).unwrap();
    let mut ours = ArchiveReader::open(&output).unwrap();
    assert!(ours.verify_integrity().unwrap());

    let Some(reference) = std::env::var_os("WUXRS_WUA") else {
        eprintln!("WUXRS_WUA is unset, not comparing");
        return;
    };
    let mut theirs = ArchiveReader::open(PathBuf::from(reference)).unwrap();
    // Walk ours; Cemu's may hold updates and DLC besides.
    let mut dirs = vec![(String::new(), zarust::ROOT_NODE)];
    let mut count = 0;
    while let Some((prefix, handle)) = dirs.pop() {
        let entries: Vec<_> = ours
            .directory_entries(handle)
            .unwrap()
            .iter()
            .map(|e| {
                (
                    String::from_utf8_lossy(e.name).into_owned(),
                    e.is_file(),
                    e.handle,
                )
            })
            .collect();
        for (name, is_file, handle) in entries {
            let path = format!("{prefix}{name}");
            if !is_file {
                dirs.push((format!("{path}/"), handle));
                continue;
            }
            let other = theirs
                .lookup(&path)
                .unwrap_or_else(|| panic!("{path} is missing from Cemu's WUA"));
            assert_eq!(
                sha1(ours.read_file_to_end(handle).unwrap().as_slice()),
                sha1(theirs.read_file_to_end(other).unwrap().as_slice()),
                "{path} differs"
            );
            count += 1;
        }
    }
    eprintln!("{count} files match");
}

#[test]
#[ignore]
fn a_real_disc_converts_to_an_installable_folder() {
    let Some(disc) = disc() else { return };
    let keys = std::env::var_os("WUXRS_KEYS")
        .map(PathBuf::from)
        .or_else(wux_rs::cemu_keys_path)
        .unwrap();
    let keys = wux_rs::load_keys(&keys).unwrap();
    let dir = tempfile::tempdir_in(disc.parent().unwrap()).unwrap();
    // Every content is checked against the disc's TMD on the way.
    wux_rs::to_nus(&disc, &dir.path().join("title"), &keys, &mut |_| {}).unwrap();
    assert!(dir.path().join("title/title.tmd").is_file());
}

#[test]
#[ignore]
fn a_real_disc_reverts_from_its_wua() {
    let Some(disc) = disc() else { return };
    let keys = std::env::var_os("WUXRS_KEYS")
        .map(PathBuf::from)
        .or_else(wux_rs::cemu_keys_path)
        .unwrap();
    let keys = wux_rs::load_keys(&keys).unwrap();
    let dir = tempfile::tempdir_in(disc.parent().unwrap()).unwrap();
    let add = std::env::var_os("WUXRS_ADD").map(PathBuf::from);
    let mut inputs = vec![disc.as_path()];
    inputs.extend(add.as_deref());
    let wua = dir.path().join("disc.wua");
    wux_rs::to_wua(&inputs, &wua, &keys, &mut |_| {}).unwrap();
    let reverted = dir.path().join("reverted");
    wux_rs::wua_to_nus(&wua, &reverted, &keys, &mut |_| {}).unwrap();

    // The disc's title as converting the disc gives it, the update as it came.
    let direct = dir.path().join("direct");
    wux_rs::to_nus(&disc, &direct, &keys, &mut |_| {}).unwrap();
    let (game, _) = wux_rs::title_id(&disc, &keys).unwrap();
    let mut originals = vec![(game, direct)];
    if let Some(add) = add {
        originals.push((wux_rs::title_id(&add, &[]).unwrap().0, add));
    }
    for (title_id, original) in originals {
        let folder = std::fs::read_dir(&reverted)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .starts_with(&format!("{title_id:016x}_v"))
            })
            .unwrap();
        for entry in std::fs::read_dir(&original).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap();
            assert_eq!(
                sha1(std::fs::File::open(folder.join(name)).unwrap()),
                sha1(std::fs::File::open(&path).unwrap()),
                "{}",
                name.to_string_lossy()
            );
        }
    }
}

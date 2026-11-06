//! Agreement with wit: our WBFSs are `wit copy --wbfs`'s byte for byte, but
//! for what wit leaves of a table of free blocks it no longer reads, and
//! what we extract from one is what wit does.
//!
//! Skipped unless wit is reachable through `$WIT` or `$PATH`. wit decrypts a
//! Wii disc with the real common key, not the synthetic disc's made-up one,
//! so only a real disc, in the ignored test below, shows a Wii WBFS is
//! wit's byte for byte.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

#[path = "../../rvz-rs/tests/common/mod.rs"]
mod common;
use common::{gamecube_disc, write};

fn wit() -> Option<PathBuf> {
    let wit = match std::env::var_os("WIT").map(PathBuf::from) {
        Some(path) => path.is_file().then_some(path),
        None => std::env::split_paths(&std::env::var_os("PATH")?)
            .map(|dir| dir.join("wit"))
            .find(|path| path.is_file()),
    };
    if wit.is_none() {
        eprintln!("skipping: no wit found (set $WIT)");
    }
    wit
}

/// Copy with wit, to WBFS or to ISO.
fn wit_copy(wit: &Path, input: &Path, output: &Path, format: &str) {
    let status = Command::new(wit)
        .args(["copy", "--quiet", format])
        .arg(input)
        .arg(output)
        .status()
        .unwrap();
    assert!(status.success(), "wit copy {format} failed");
}

/// Compare two files from `offset` up to `size`, a piece at a time, as a
/// disc is too big to read whole.
fn assert_same_range(ours: &Path, theirs: &Path, offset: u64, size: u64, what: &str) {
    let mut ours = std::fs::File::open(ours).unwrap();
    let mut theirs = std::fs::File::open(theirs).unwrap();
    std::io::Seek::seek(&mut ours, std::io::SeekFrom::Start(offset)).unwrap();
    std::io::Seek::seek(&mut theirs, std::io::SeekFrom::Start(offset)).unwrap();
    let (mut a, mut b) = (vec![0; 0x100_0000], vec![0; 0x100_0000]);
    let mut offset = offset;
    while offset < size {
        let length = (size - offset).min(a.len() as u64) as usize;
        std::io::Read::read_exact(&mut ours, &mut a[..length]).unwrap();
        std::io::Read::read_exact(&mut theirs, &mut b[..length]).unwrap();
        assert!(a[..length] == b[..length], "{what}: differs at {offset:#x}");
        offset += length as u64;
    }
}

fn assert_same_file(ours: &Path, theirs: &Path, what: &str) {
    let size = std::fs::metadata(ours).unwrap().len();
    let other = std::fs::metadata(theirs).unwrap().len();
    assert_eq!(size, other, "{what}: our size against wit's");
    assert_same_range(ours, theirs, 0, size, what);
}

/// Where wit's leftover table of free blocks can lie: from where it lays the
/// table out for the largest disc to the end of the first block. The table
/// it reads lies there too, all zeros, every block it stores being used.
const LEFTOVER: std::ops::Range<usize> = 0x1F_FC00..0x20_0000;

/// Our WBFS is wit's, but for the leftover table, which we leave zeroed.
fn assert_same_wbfs(ours: &Path, theirs: &Path) {
    let size = std::fs::metadata(ours).unwrap().len();
    assert_eq!(
        size,
        std::fs::metadata(theirs).unwrap().len(),
        "our size against wit's"
    );
    let mut a = vec![0; 0x20_0000];
    let mut b = vec![0; 0x20_0000];
    std::io::Read::read_exact(&mut std::fs::File::open(ours).unwrap(), &mut a).unwrap();
    std::io::Read::read_exact(&mut std::fs::File::open(theirs).unwrap(), &mut b).unwrap();
    assert!(
        a[LEFTOVER].iter().all(|&byte| byte == 0),
        "our free table is zeroed"
    );
    b[LEFTOVER].fill(0);
    assert!(a == b, "first block");
    assert_same_range(ours, theirs, 0x20_0000, size, "created");
}

#[test]
fn a_gamecube_wbfs_is_wits() {
    let Some(wit) = wit() else { return };
    let dir = TempDir::new().unwrap();
    let iso = dir.path().join("disc.iso");
    let ours = dir.path().join("ours.wbfs");
    let theirs = dir.path().join("theirs.wbfs");
    write(&iso, &gamecube_disc(0x80_0000));
    wbfs_rs::create(&iso, &ours, &[], &mut |_| {}).unwrap();
    wit_copy(&wit, &iso, &theirs, "--wbfs");
    assert_same_wbfs(&ours, &theirs);
}

#[test]
fn a_gamecube_disc_extracts_as_wit_extracts_it() {
    let Some(wit) = wit() else { return };
    let dir = TempDir::new().unwrap();
    let iso = dir.path().join("disc.iso");
    let wbfs = dir.path().join("disc.wbfs");
    let ours = dir.path().join("ours.iso");
    let theirs = dir.path().join("theirs.iso");
    write(&iso, &gamecube_disc(0x80_0000));
    wit_copy(&wit, &iso, &wbfs, "--wbfs");
    wbfs_rs::extract(&wbfs, &ours, &mut |_| {}).unwrap();
    wit_copy(&wit, &wbfs, &theirs, "--iso");
    // wit pads a GameCube disc to a single-layer Wii disc's size: the same
    // up to a GameCube disc's, and zeros after
    let size = std::fs::metadata(&ours).unwrap().len();
    assert_eq!(size, 1_459_978_240);
    assert_same_range(&ours, &theirs, 0, size, "extracted");
}

/// A real Wii or GameCube disc from `$WBFS_RS_REAL`, an RVZ, so that no key
/// is needed: its WBFS, and what that extracts to, are wit's. Several
/// gigabytes are written to the temp directory.
#[test]
#[ignore = "writes a real disc several times over"]
fn a_real_disc_is_written_and_extracted_as_wit_does() {
    let Some(wit) = wit() else { return };
    let Some(rvz) = std::env::var_os("WBFS_RS_REAL").map(PathBuf::from) else {
        eprintln!("skipping: set $WBFS_RS_REAL to an RVZ");
        return;
    };
    let dir = TempDir::new_in(std::env::temp_dir()).unwrap();
    let iso = dir.path().join("disc.iso");
    let ours = dir.path().join("ours.wbfs");
    let theirs = dir.path().join("theirs.wbfs");
    rvz_rs::decompress(&rvz, &iso, &mut |_| {}).unwrap();
    wit_copy(&wit, &iso, &theirs, "--wbfs");
    std::fs::remove_file(&iso).unwrap();
    wbfs_rs::create(&rvz, &ours, &[], &mut |_| {}).unwrap();
    assert_same_wbfs(&ours, &theirs);
    std::fs::remove_file(&ours).unwrap();

    let ours = dir.path().join("ours.iso");
    let theirs_iso = dir.path().join("theirs.iso");
    wbfs_rs::extract(&theirs, &ours, &mut |_| {}).unwrap();
    wit_copy(&wit, &theirs, &theirs_iso, "--iso");
    assert_same_file(&ours, &theirs_iso, "extracted");
}

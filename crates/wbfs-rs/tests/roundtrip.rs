//! WBFSs of synthetic discs: what the disc uses survives the trip there and
//! back, everything else comes back as zeros, and what is not a disc or not
//! a WBFS is refused.

use std::path::Path;

use tempfile::TempDir;
use wbfs_rs::Error;

#[path = "../../rvz-rs/tests/common/mod.rs"]
mod common;
use common::{COMMON_KEY, gamecube_disc, wii_disc, write};

const CLUSTER: usize = 0x8000;

/// Write `disc` to WBFS and back, checking the disc that comes back against
/// the clusters `keys` say it uses, and return the WBFS.
fn round_trip(dir: &Path, disc: &[u8], keys: &[[u8; 16]], size: u64) -> Vec<u8> {
    let iso = dir.join("disc.iso");
    let wbfs = dir.join("disc.wbfs");
    let back = dir.join("back.iso");
    write(&iso, disc);
    let mut consumed = 0;
    let stats = wbfs_rs::create(&iso, &wbfs, keys, &mut |n| consumed += n).unwrap();
    assert_eq!(consumed, disc.len() as u64, "progress adds up to the input");
    assert_eq!(stats.output_size, std::fs::metadata(&wbfs).unwrap().len());
    assert!(wbfs_rs::is_wbfs(&wbfs).unwrap());

    let mut consumed = 0;
    wbfs_rs::extract(&wbfs, &back, &mut |n| consumed += n).unwrap();
    assert_eq!(consumed, stats.output_size, "progress adds up to the input");
    assert_eq!(std::fs::metadata(&back).unwrap().len(), size);
    // Only as much as the disc held: past it, a hole up to the size above
    let mut file = std::fs::File::open(&back).unwrap();
    let mut back = vec![0; disc.len()];
    std::io::Read::read_exact(&mut file, &mut back).unwrap();
    let used = rvz_rs::used_clusters(&iso, keys).unwrap();
    for (i, (ours, theirs)) in back.chunks(CLUSTER).zip(disc.chunks(CLUSTER)).enumerate() {
        if used[i] {
            assert_eq!(ours, theirs, "used cluster {i}");
        } else {
            assert!(ours.iter().all(|&byte| byte == 0), "unused cluster {i}");
        }
    }
    std::fs::read(&wbfs).unwrap()
}

#[test]
fn a_gamecube_disc_round_trips_as_long_as_a_gamecube_disc() {
    let dir = TempDir::new().unwrap();
    let wbfs = round_trip(dir.path(), &gamecube_disc(0x80_0000), &[], 1_459_978_240);
    // Its header and the one block holding all it uses
    assert_eq!(wbfs.len(), 2 * 0x20_0000);
}

#[test]
fn a_wii_disc_round_trips_as_long_as_a_single_layer_one() {
    let dir = TempDir::new().unwrap();
    round_trip(dir.path(), &wii_disc(), &[COMMON_KEY], 143_432 * 0x8000);
}

#[test]
fn a_wii_disc_needs_its_common_key() {
    let dir = TempDir::new().unwrap();
    let iso = dir.path().join("disc.iso");
    let wbfs = dir.path().join("disc.wbfs");
    write(&iso, &wii_disc());
    for keys in [&[][..], &[[0x22; 16]][..]] {
        let error = wbfs_rs::create(&iso, &wbfs, keys, &mut |_| {}).unwrap_err();
        assert!(matches!(error, Error::MissingKey(_)), "{error}");
        assert!(!wbfs.exists(), "a failed run leaves nothing behind");
    }
}

#[test]
fn an_rvz_needs_no_key_and_gives_the_same_wbfs() {
    let dir = TempDir::new().unwrap();
    let disc = wii_disc();
    let from_iso = round_trip(dir.path(), &disc, &[COMMON_KEY], 143_432 * 0x8000);
    let iso = dir.path().join("disc.iso");
    let rvz = dir.path().join("disc.rvz");
    let wbfs = dir.path().join("rvz.wbfs");
    let options = rvz_rs::Options::default();
    rvz_rs::compress(&iso, &rvz, &options, &[COMMON_KEY], &mut |_| {}).unwrap();
    let mut consumed = 0;
    wbfs_rs::create(&rvz, &wbfs, &[], &mut |n| consumed += n).unwrap();
    assert_eq!(consumed, std::fs::metadata(&rvz).unwrap().len());
    assert!(std::fs::read(&wbfs).unwrap() == from_iso);
}

#[test]
fn refuses_what_is_not_a_disc() {
    let dir = TempDir::new().unwrap();
    let iso = dir.path().join("noise.iso");
    let wbfs = dir.path().join("noise.wbfs");
    write(&iso, &common::noise(0x10_0000, 7));
    let error = wbfs_rs::create(&iso, &wbfs, &[], &mut |_| {}).unwrap_err();
    assert!(matches!(error, Error::Unsupported(_)), "{error}");
    assert!(!wbfs.exists());
}

#[test]
fn refuses_what_is_not_a_wbfs() {
    let dir = TempDir::new().unwrap();
    let iso = dir.path().join("disc.iso");
    let back = dir.path().join("back.iso");
    write(&iso, &gamecube_disc(0x80_0000));
    assert!(!wbfs_rs::is_wbfs(&iso).unwrap());
    let error = wbfs_rs::extract(&iso, &back, &mut |_| {}).unwrap_err();
    assert!(matches!(error, Error::BadMagic { .. }), "{error}");
    assert!(!back.exists());
}

#[test]
fn refuses_a_damaged_wbfs() {
    let dir = TempDir::new().unwrap();
    let iso = dir.path().join("disc.iso");
    let wbfs = dir.path().join("disc.wbfs");
    let damaged = dir.path().join("damaged.wbfs");
    let back = dir.path().join("back.iso");
    write(&iso, &gamecube_disc(0x80_0000));
    wbfs_rs::create(&iso, &wbfs, &[], &mut |_| {}).unwrap();
    let good = std::fs::read(&wbfs).unwrap();

    // Cut short
    write(&damaged, &good[..good.len() - 0x200]);
    let error = wbfs_rs::extract(&damaged, &back, &mut |_| {}).unwrap_err();
    assert!(matches!(error, Error::Corrupt(_)), "{error}");

    // A block stored past the end of the file
    let mut bad = good.clone();
    bad[0x302..0x304].copy_from_slice(&2u16.to_be_bytes());
    write(&damaged, &bad);
    let error = wbfs_rs::extract(&damaged, &back, &mut |_| {}).unwrap_err();
    assert!(matches!(error, Error::Corrupt(_)), "{error}");

    // Blocks smaller than a sector
    let mut bad = good;
    bad[9] = 8;
    write(&damaged, &bad);
    let error = wbfs_rs::extract(&damaged, &back, &mut |_| {}).unwrap_err();
    assert!(matches!(error, Error::Corrupt(_)), "{error}");
    assert!(!back.exists());
}

/// A synthetic GameCube disc without a boot DOL, as homebrew discs can be.
fn gamecube_disc_without_dol() -> Vec<u8> {
    let mut disc = gamecube_disc(0x80_0000);
    disc[0x420..0x424].fill(0);
    disc
}

#[test]
fn a_disc_without_a_dol_is_kept_whole() {
    // Its own apploader loads code from where no file points, so all of it
    // comes back, in all four blocks it spans
    let dir = TempDir::new().unwrap();
    let disc = gamecube_disc_without_dol();
    let wbfs = round_trip(dir.path(), &disc, &[], 1_459_978_240);
    assert_eq!(wbfs.len(), 5 * 0x20_0000);
    let back = std::fs::read(dir.path().join("back.iso")).unwrap();
    assert!(back[..disc.len()] == disc[..]);
    // Where scrubbing it for an RVZ refuses it, as Dolphin does
    let iso = dir.path().join("disc.iso");
    let rvz = dir.path().join("disc.rvz");
    let options = rvz_rs::Options {
        scrub: true,
        ..Default::default()
    };
    let error = rvz_rs::compress(&iso, &rvz, &options, &[], &mut |_| {}).unwrap_err();
    assert!(matches!(error, rvz_rs::Error::Corrupt(_)), "{error}");
}

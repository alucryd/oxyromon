//! A PS3 disc as Redump dumps it, its regions between the plain ones
//! sector 0 lists encrypted, comes back byte for byte: decrypted with the
//! disc key when one of the keys given is its, so what is under the
//! encryption compresses, and as it is otherwise.

// They write the OREs they read
#![cfg(feature = "write")]

use ore_rs::{Options, Reader};
use tempfile::TempDir;
use test_support::ps3;

fn setup() -> (TempDir, ps3::Disc, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let disc = ps3::disc();
    let iso = dir.path().join("Test Game (USA).iso");
    std::fs::write(&iso, &disc.iso).unwrap();
    (dir, disc, iso)
}

/// Compress `iso` with `keys`, check it reads back whole and in pieces,
/// and return the ORE's size.
fn round_trip(dir: &TempDir, iso: &std::path::Path, disc: &ps3::Disc, keys: &[[u8; 16]]) -> u64 {
    let ore = dir.path().join("Test Game (USA).ore");
    let stats = ore_rs::compress(iso, &ore, &Options::default(), keys, &mut |_| {}).unwrap();
    let mut reader = Reader::open(&ore).unwrap();
    assert_eq!(reader.platform(), "ps3");
    // across the plain region's end and the encrypted region's sectors,
    // and within one
    for (at, len) in [
        (39 * 2048 + 100, 5000),
        (48 * 2048 + 7, 3),
        (98 * 2048, 4096 + 10),
    ] {
        let mut buf = vec![0; len];
        reader.read_at(at, &mut buf).unwrap();
        assert!(buf == disc.iso[at as usize..at as usize + len], "at {at}");
    }
    let back = dir.path().join("back.iso");
    ore_rs::decompress(&ore, &back, &mut |_| {}).unwrap();
    assert!(
        std::fs::read(&back).unwrap() == disc.iso,
        "the disc differs"
    );
    stats.output_size
}

#[test]
fn a_disc_whose_key_is_given_is_stored_decrypted() {
    let (dir, disc, iso) = setup();
    // its key among others, which are told wrong
    let decrypted = round_trip(&dir, &iso, &disc, &[[0x11; 16], ps3::DISC_KEY]);
    let encrypted = round_trip(&dir, &iso, &disc, &[[0x11; 16]]);
    // what the encryption hid compresses
    assert!(
        decrypted + 4000 < encrypted,
        "decrypted {decrypted}, encrypted {encrypted}"
    );
}

#[test]
fn a_disc_without_its_key_comes_back_as_it_is() {
    let (dir, disc, iso) = setup();
    round_trip(&dir, &iso, &disc, &[]);
}

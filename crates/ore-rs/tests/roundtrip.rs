//! Every disc comes back out of an ORE byte for byte, whatever the profile
//! and codec, and a damaged ORE fails rather than give back another disc.

use std::path::Path;

use ore_rs::{Codec, Options, Profile, Reader};
use sha1::{Digest, Sha1};
use tempfile::TempDir;

mod common;
use common::{COMMON_KEY, WII_PARTITION_DATA, gamecube_disc, noise, wii_disc, write};

const OPTIONS: [Options; 3] = [
    Options {
        profile: Profile::Stream,
        codec: Codec::Zstd,
    },
    Options {
        profile: Profile::Archive,
        codec: Codec::Zstd,
    },
    Options {
        profile: Profile::Archive,
        codec: Codec::Lzma2,
    },
];

/// Compress `disc`, check the ORE decompresses back to it, and return the ORE.
fn round_trip(dir: &Path, disc: &[u8], options: &Options, keys: &[[u8; 16]]) -> Vec<u8> {
    let (iso, ore, out) = (
        dir.join("disc.iso"),
        dir.join("disc.ore"),
        dir.join("out.iso"),
    );
    write(&iso, disc);
    let mut consumed = 0;
    ore_rs::compress(&iso, &ore, options, keys, &mut |n| consumed += n).unwrap();
    assert_eq!(consumed, disc.len() as u64, "progress adds up to the input");
    let ore_size = std::fs::metadata(&ore).unwrap().len();
    let mut consumed = 0;
    ore_rs::decompress(&ore, &out, &mut |n| consumed += n).unwrap();
    assert_eq!(consumed, ore_size, "progress adds up to the input");
    assert!(
        std::fs::read(&out).unwrap() == disc,
        "{options:?} does not round-trip"
    );
    std::fs::read(&ore).unwrap()
}

/// A GameCube disc with a file repeated, as discs repeat content.
fn repeating_disc() -> Vec<u8> {
    let mut disc = gamecube_disc(0x580000);
    let file = noise(0x60000, 99);
    disc[0x200000..0x260000].copy_from_slice(&file);
    disc[0x400000..0x460000].copy_from_slice(&file);
    disc
}

#[test]
fn a_gamecube_disc_round_trips_with_every_profile_and_codec() {
    let dir = TempDir::new().unwrap();
    let disc = repeating_disc();
    for options in &OPTIONS {
        let ore = round_trip(dir.path(), &disc, options, &[]);
        // The repeated file and the padding cost nothing: what is left is
        // the noise the disc holds, once
        assert!(
            ore.len() < 0x60000 + 0x70000,
            "{options:?}: {} bytes",
            ore.len()
        );
    }
}

#[test]
fn a_wii_disc_round_trips_its_hash_exception_included() {
    let dir = TempDir::new().unwrap();
    let disc = wii_disc();
    for options in &OPTIONS {
        round_trip(dir.path(), &disc, options, &[COMMON_KEY]);
    }
}

#[test]
fn a_wii_disc_needs_its_common_key() {
    let dir = TempDir::new().unwrap();
    let (iso, ore) = (dir.path().join("disc.iso"), dir.path().join("disc.ore"));
    write(&iso, &wii_disc());
    let error = ore_rs::compress(&iso, &ore, &Options::default(), &[], &mut |_| {}).unwrap_err();
    assert!(matches!(error, ore_rs::Error::MissingKey(_)), "{error}");
    assert!(!ore.exists() && !dir.path().join("disc.ore.part").exists());
}

#[test]
fn reads_anywhere_and_records_the_disc_hashes() {
    let dir = TempDir::new().unwrap();
    let disc = wii_disc();
    round_trip(dir.path(), &disc, &Options::default(), &[COMMON_KEY]);
    let mut reader = Reader::open(&dir.path().join("disc.ore")).unwrap();
    assert_eq!(reader.size(), disc.len() as u64);
    for (offset, length) in [
        (0, 0x10),
        (WII_PARTITION_DATA - 7, 0x9000),
        (0x123456, 0x40000),
    ] {
        let mut buf = vec![0; length];
        reader.read_at(offset as u64, &mut buf).unwrap();
        assert!(buf == disc[offset..offset + length], "read at {offset:#x}");
    }
    let mut buf = [0; 2];
    assert!(reader.read_at(disc.len() as u64 - 1, &mut buf).is_err());
    let sha1: Vec<u8> = Sha1::digest(&disc).to_vec();
    assert!(
        reader
            .identity()
            .any(|(view, algo, value)| view == "logical" && algo == "sha-1" && value == sha1)
    );
}

#[test]
fn a_damaged_ore_fails() {
    let dir = TempDir::new().unwrap();
    let ore = round_trip(dir.path(), &repeating_disc(), &Options::default(), &[]);
    let (damaged, out) = (
        dir.path().join("damaged.ore"),
        dir.path().join("damaged.iso"),
    );
    // A byte of the first group, of the index, of the metadata, and the file cut short
    let index = u64::from_be_bytes(ore[48..56].try_into().unwrap()) as usize;
    let meta = u64::from_be_bytes(ore[72..80].try_into().unwrap()) as usize;
    for at in [200, index + 1, meta + 3] {
        let mut bytes = ore.clone();
        bytes[at] ^= 0x40;
        write(&damaged, &bytes);
        let result =
            Reader::open(&damaged).and_then(|_| ore_rs::decompress(&damaged, &out, &mut |_| {}));
        assert!(result.is_err(), "a byte flipped at {at} went unnoticed");
        assert!(!out.exists());
    }
    write(&damaged, &ore[..ore.len() - 1]);
    assert!(Reader::open(&damaged).is_err());
}

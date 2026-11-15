//! What a reader of this layout must take from a later writer: a higher
//! `spec_version`, optional flags it does not know, regions it does not
//! read, bytes after what it reads. And what it must refuse: required
//! flags it does not know, and a later layout.

use std::path::{Path, PathBuf};

use ore_rs::{Error, Options, Reader};
use tempfile::TempDir;

/// An ORE of 64 KiB of noise, named a NES ROM: its path and the ROM.
fn ore(dir: &Path) -> (PathBuf, Vec<u8>) {
    let mut state = 13u64;
    let rom: Vec<u8> = (0..64 << 10)
        .map(|_| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (state >> 56) as u8
        })
        .collect();
    let input = dir.join("Game.nes");
    std::fs::write(&input, &rom).unwrap();
    let ore = dir.join("Game.ore");
    let options = Options {
        platform: Some("nes"),
        ..Options::default()
    };
    ore_rs::compress(&input, &ore, &options, &[], &mut |_| {}).unwrap();
    (ore, rom)
}

/// The ORE at `ore` with its header edited by `edit`, at `name` beside it.
fn edited(ore: &Path, name: &str, edit: impl FnOnce(&mut Vec<u8>)) -> PathBuf {
    let mut bytes = std::fs::read(ore).unwrap();
    edit(&mut bytes);
    let path = ore.with_file_name(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn reads_as(path: &Path, rom: &[u8]) {
    let mut reader = Reader::open(path).unwrap();
    let mut back = vec![0; rom.len()];
    reader.read_at(0, &mut back).unwrap();
    assert!(back == rom);
}

#[test]
fn what_a_later_writer_adds_is_read_past() {
    let dir = TempDir::new().unwrap();
    let (ore, rom) = ore(dir.path());
    for (name, edit) in [
        // An addition to this layout
        (
            "version.ore",
            (|b: &mut Vec<u8>| b[4..6].copy_from_slice(&2u16.to_be_bytes())) as fn(&mut Vec<u8>),
        ),
        // An optional feature this build does not know
        ("optional.ore", |b| {
            b[12..16].copy_from_slice(&(1u32 << 31).to_be_bytes())
        }),
        // Fixity and AUX regions it does not read
        ("regions.ore", |b| b[152..184].fill(0x5A)),
        // A longer header, its regions where their offsets say
        ("header.ore", |b| {
            b[6..8].copy_from_slice(&256u16.to_be_bytes())
        }),
        // A region appended after what it reads, as parity would be
        ("appended.ore", |b| {
            b.extend(std::iter::repeat_n(0xA5, 64 << 10))
        }),
    ] {
        let path = edited(&ore, name, edit);
        reads_as(&path, &rom);
        // And decompresses whole, its hashes checked
        let back = dir.path().join("back.nes");
        ore_rs::decompress(&path, &back, &mut |_| {}).unwrap();
        assert!(std::fs::read(&back).unwrap() == rom, "{name}");
    }
}

#[test]
fn what_a_reader_cannot_do_without_is_refused() {
    let dir = TempDir::new().unwrap();
    let (ore, _) = ore(dir.path());
    let required = edited(&ore, "required.ore", |b| {
        b[8..12].copy_from_slice(&(1u32 << 31).to_be_bytes())
    });
    assert!(matches!(
        Reader::open(&required),
        Err(Error::Unsupported(_))
    ));
    let later = edited(&ore, "later.ore", |b| b[3] = b'2');
    match Reader::open(&later) {
        Err(Error::Unsupported(message)) => assert!(message.contains("layout 2"), "{message}"),
        other => panic!("{:?}", other.err()),
    }
    let short = edited(&ore, "short.ore", |b| {
        b[6..8].copy_from_slice(&128u16.to_be_bytes())
    });
    assert!(matches!(Reader::open(&short), Err(Error::Unsupported(_))));
    let other = edited(&ore, "other.ore", |b| b[0..4].copy_from_slice(b"RIFF"));
    assert!(matches!(Reader::open(&other), Err(Error::BadMagic { .. })));
}

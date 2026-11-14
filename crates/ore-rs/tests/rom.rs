//! A Game Boy or Game Boy Advance ROM comes back out of an ORE byte for
//! byte, under its name; a ROM whose header its console would refuse is
//! refused.

use std::path::{Path, PathBuf};

use ore_rs::{Options, Reader};
use tempfile::TempDir;

const GB_LOGO: [u8; 48] = [
    0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00, 0x0D,
    0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD, 0xD9, 0x99,
    0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
];

/// 256 KiB of a ROM: noise, then 0xFF padding as cartridges have.
fn rom(dir: &Path, name: &str, header: impl FnOnce(&mut [u8])) -> PathBuf {
    let mut state = 7u64;
    let mut bytes: Vec<u8> = (0..128 << 10)
        .map(|_| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (state >> 56) as u8
        })
        .collect();
    bytes.resize(256 << 10, 0xFF);
    header(&mut bytes);
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn gb(rom: &mut [u8]) {
    rom[0x104..0x134].copy_from_slice(&GB_LOGO);
    rom[0x143] = 0x80;
}

fn gba(rom: &mut [u8]) {
    rom[0xB2] = 0x96;
    rom[0xBD] = rom[0xA0..0xBD]
        .iter()
        .fold(0u8, |sum, &b| sum.wrapping_sub(b))
        .wrapping_sub(0x19);
}

#[test]
fn a_game_boy_and_a_game_boy_advance_rom_round_trip_under_their_names() {
    let dir = TempDir::new().unwrap();
    for (name, header) in [
        ("Game (USA).gbc", gb as fn(&mut [u8])),
        ("Game (Europe).gba", gba),
    ] {
        let input = rom(dir.path(), name, header);
        let ore = dir.path().join("game.ore");
        let stats = ore_rs::compress(&input, &ore, &Options::default(), &[], &mut |_| {}).unwrap();
        // The noise, and next to nothing of the padding
        assert!(stats.output_size < 140 << 10, "{}", stats.output_size);
        assert_eq!(Reader::open(&ore).unwrap().file_name(), Some(name));
        let back = dir.path().join("back.rom");
        ore_rs::decompress(&ore, &back, &mut |_| {}).unwrap();
        assert!(std::fs::read(&input).unwrap() == std::fs::read(&back).unwrap());
    }
}

#[test]
fn a_rom_whose_header_does_not_check_is_refused() {
    let dir = TempDir::new().unwrap();
    // A logo off by a bit, and a checksum off by one
    let bad_gb = rom(dir.path(), "a.gb", |rom| {
        gb(rom);
        rom[0x110] ^= 1;
    });
    let bad_gba = rom(dir.path(), "a.gba", |rom| {
        gba(rom);
        rom[0xBD] = rom[0xBD].wrapping_add(1);
    });
    for input in [bad_gb, bad_gba] {
        let ore = dir.path().join("a.ore");
        assert!(ore_rs::compress(&input, &ore, &Options::default(), &[], &mut |_| {}).is_err());
        assert!(!ore.exists());
    }
}

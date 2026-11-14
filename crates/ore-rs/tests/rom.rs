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

fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            match entry.metadata().unwrap() {
                m if m.is_dir() => dir_size(&entry.path()),
                m => m.len(),
            }
        })
        .sum()
}

#[test]
fn a_batch_writes_many_roms_to_a_store_as_one_write() {
    let dir = TempDir::new().unwrap();
    // Ten revisions of a ROM, a few bytes apart, and one that is no ROM
    let mut roms = Vec::new();
    for revision in 0..10u8 {
        roms.push(rom(
            dir.path(),
            &format!("Game (Rev {revision}).gb"),
            |rom| {
                gb(rom);
                rom[0x8000 + revision as usize * 0x1000] = revision;
            },
        ));
    }
    let not_a_rom = dir.path().join("not.gb");
    std::fs::write(&not_a_rom, vec![0x55; 0x8000]).unwrap();
    let options = Options::default();

    // One write a ROM, then one batch
    let mut sizes = Vec::new();
    for batched in [false, true] {
        let games = dir.path().join(if batched { "batch" } else { "single" });
        let store = games.join(".ore-store");
        ore_rs::create_store(&store, &Default::default()).unwrap();
        let ore = |rom: &Path| games.join(rom.file_name().unwrap()).with_extension("ore");
        if batched {
            let mut batch = ore_rs::StoreBatch::open(&store, &options).unwrap();
            for (i, rom) in roms.iter().enumerate() {
                batch
                    .compress(rom, &ore(rom), &options, &[], &mut |_| {})
                    .unwrap();
                if i == 4 {
                    assert!(
                        batch
                            .compress(&not_a_rom, &ore(&not_a_rom), &options, &[], &mut |_| {})
                            .is_err()
                    );
                }
            }
            // Nothing lands before the commit
            assert!(!ore(&roms[0]).exists());
            batch.commit().unwrap();
        } else {
            for rom in &roms {
                ore_rs::compress_to_store(rom, &ore(rom), &store, &options, &[], &mut |_| {})
                    .unwrap();
            }
        }
        for rom in &roms {
            let back = dir.path().join("back.gb");
            ore_rs::decompress(&ore(rom), &back, &mut |_| {}).unwrap();
            assert!(std::fs::read(rom).unwrap() == std::fs::read(&back).unwrap());
        }
        assert!(!ore(&not_a_rom).exists());
        sizes.push(dir_size(&games));
    }
    // One segment either way, a write folding the last into its own; the
    // batch the smaller, its groups shared by the ROMs rather than closed a
    // write
    for which in ["single", "batch"] {
        let store = dir.path().join(which).join(".ore-store");
        let segments = std::fs::read_dir(&store)
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|e| e == "ores")
            })
            .count();
        assert_eq!(segments, 1, "{which}");
    }
    assert!(sizes[1] < sizes[0], "{sizes:?}");

    // A compaction packs the ROMs written one at a time together, as the
    // batch did
    let single = dir.path().join("single");
    let compacted = ore_rs::compact(&single.join(".ore-store"), &options, 0.5).unwrap();
    assert_eq!(compacted.games, 10);
    let after = dir_size(&single);
    assert!(
        after <= sizes[1] * 101 / 100,
        "{after} against the batch's {}",
        sizes[1]
    );
    for rom in &roms {
        let back = dir.path().join("back.gb");
        let ore = single.join(rom.file_name().unwrap()).with_extension("ore");
        ore_rs::decompress(&ore, &back, &mut |_| {}).unwrap();
        assert!(std::fs::read(rom).unwrap() == std::fs::read(&back).unwrap());
    }

    // A batch dropped before its commit writes nothing, and the store reads on
    let games = dir.path().join("batch");
    let mut batch = ore_rs::StoreBatch::open(&games.join(".ore-store"), &options).unwrap();
    let extra = rom(dir.path(), "Extra.gba", gba);
    batch
        .compress(&extra, &games.join("Extra.ore"), &options, &[], &mut |_| {})
        .unwrap();
    drop(batch);
    assert!(!games.join("Extra.ore").exists());
    assert!(
        ore_rs::check_store(&games.join(".ore-store"), false)
            .unwrap()
            .lost_groups
            == 0
    );
}

//! A PlayStation 2 DVD comes back out of an ORE byte for byte, its zeros
//! and repeated data costing next to nothing; an ISO that does not boot a
//! PlayStation 2 is refused.

use std::path::Path;

use ore_rs::{Options, Reader};
use tempfile::TempDir;

const SECTOR: usize = 2048;

fn noise(len: usize, mut state: u64) -> Vec<u8> {
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

/// An ISO 9660 directory record.
fn record(name: &[u8], lba: u32, size: u32, directory: bool) -> Vec<u8> {
    let len = 33 + name.len() + (1 - name.len() % 2);
    let mut out = vec![0; len];
    out[0] = len as u8;
    out[2..6].copy_from_slice(&lba.to_le_bytes());
    out[6..10].copy_from_slice(&lba.to_be_bytes());
    out[10..14].copy_from_slice(&size.to_le_bytes());
    out[14..18].copy_from_slice(&size.to_be_bytes());
    out[25] = if directory { 2 } else { 0 };
    out[32] = name.len() as u8;
    out[33..33 + name.len()].copy_from_slice(name);
    out
}

/// A DVD of 2,000 sectors in `dir`: its file system, a `SYSTEM.CNF` holding
/// `cnf`, noise, a long run of zeros and one block of noise twice.
fn disc(dir: &Path, cnf: &str) -> std::path::PathBuf {
    let mut iso = vec![0; 2000 * SECTOR];
    let pvd = &mut iso[16 * SECTOR..17 * SECTOR];
    pvd[0] = 1;
    pvd[1..6].copy_from_slice(b"CD001");
    pvd[156..190].copy_from_slice(&record(&[0], 18, SECTOR as u32, true));
    let mut root = record(&[0], 18, SECTOR as u32, true);
    root.extend(record(&[1], 18, SECTOR as u32, true));
    root.extend(record(b"SYSTEM.CNF;1", 19, cnf.len() as u32, false));
    iso[18 * SECTOR..18 * SECTOR + root.len()].copy_from_slice(&root);
    iso[19 * SECTOR..19 * SECTOR + cnf.len()].copy_from_slice(cnf.as_bytes());
    iso[20 * SECTOR..600 * SECTOR].copy_from_slice(&noise(580 * SECTOR, 1));
    // 600..1400 left zero
    let block = noise(200 * SECTOR, 2);
    iso[1400 * SECTOR..1600 * SECTOR].copy_from_slice(&block);
    iso[1800 * SECTOR..2000 * SECTOR].copy_from_slice(&block);
    let path = dir.join("game.iso");
    std::fs::write(&path, iso).unwrap();
    path
}

#[test]
fn a_ps2_dvd_round_trips_its_zeros_and_repeats_stored_once() {
    let dir = TempDir::new().unwrap();
    let iso = disc(
        dir.path(),
        "BOOT2 = cdrom0:\\SLUS_200.00;1\r\nVER = 1.00\r\n",
    );
    let ore = dir.path().join("game.ore");
    let mut consumed = 0;
    let stats =
        ore_rs::compress(&iso, &ore, &Options::default(), &[], &mut |n| consumed += n).unwrap();
    assert_eq!(consumed, stats.input_size);
    // The noise once, 780 sectors: the zeros and the second copy are free
    assert!(
        stats.output_size < 790 * SECTOR as u64,
        "{}",
        stats.output_size
    );
    assert!(!Reader::open(&ore).unwrap().is_cd());

    let back = dir.path().join("back.iso");
    ore_rs::decompress(&ore, &back, &mut |_| {}).unwrap();
    assert!(std::fs::read(&iso).unwrap() == std::fs::read(&back).unwrap());
}

#[test]
fn an_iso_that_boots_no_ps2_is_refused() {
    let dir = TempDir::new().unwrap();
    let iso = disc(dir.path(), "BOOT = cdrom:\\SLUS_000.00;1\r\n");
    let ore = dir.path().join("game.ore");
    assert!(ore_rs::compress(&iso, &ore, &Options::default(), &[], &mut |_| {}).is_err());
    assert!(!ore.exists());
}

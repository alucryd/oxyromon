//! A PlayStation 2 DVD comes back out of an ORE byte for byte, its zeros
//! and repeated data costing next to nothing, its audio coded as audio,
//! alone and through a store; an ISO that does not boot a PlayStation 2 is
//! refused. A PSP UMD is cut as one.

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

/// SPU ADPCM of a wavering tone, `blocks` 16-byte blocks of it.
fn spu(blocks: usize, seed: u64) -> Vec<u8> {
    const K0: [i32; 3] = [0, 60, 115];
    const K1: [i32; 3] = [0, 0, -52];
    let wobble = noise(blocks * 28, seed);
    let (mut old, mut older) = (0i32, 0i32);
    let mut out = Vec::new();
    for b in 0..blocks {
        let (shift, filter) = (6, 2);
        let mut block = [0u8; 16];
        block[0] = (filter << 4 | shift) as u8;
        for j in 0..28 {
            let n = b * 28 + j;
            let x = ((n as f64 * 0.02).sin() * 6000.0) as i32 + wobble[n] as i32 * 8;
            let pred = (old * K0[filter] + older * K1[filter] + 32) >> 6;
            let t = ((x - pred) >> (12 - shift)).clamp(-8, 7);
            block[2 + j / 2] |= ((t & 15) as u8) << ((j & 1) * 4);
            older = old;
            old = (((t << 12) >> shift) + pred).clamp(-32768, 32767);
        }
        out.extend_from_slice(&block);
    }
    out
}

/// A DVD with audio on it: a `SShd` stream of 16-bit stereo PCM, its
/// channels 512 bytes a turn, and three `VAGp` streams close together,
/// their headers whole or, to measure without, garbled.
fn audio_disc(dir: &Path, headed: bool) -> std::path::PathBuf {
    let iso = disc(dir, "BOOT2 = cdrom0:\\SLUS_200.00;1\r\n");
    let mut bytes = std::fs::read(&iso).unwrap();
    bytes.resize(3000 * SECTOR, 0);
    let wobble = noise(1 << 20, 3);
    let mut pcm = Vec::new();
    for period in 0..1024 {
        for channel in 0..2 {
            for i in 0..256 {
                let n = period * 256 + i;
                let x = ((n as f64 * 0.01 + channel as f64).sin() * 9000.0) as i16
                    + (wobble[n] as i16 >> 4);
                pcm.extend_from_slice(&x.to_le_bytes());
            }
        }
    }
    let at = 650 * SECTOR;
    bytes[at..at + 4].copy_from_slice(if headed { b"SShd" } else { b"XXhd" });
    bytes[at + 4..at + 8].copy_from_slice(&0x18u32.to_le_bytes());
    for (i, v) in [1u32, 48_000, 2, 512].iter().enumerate() {
        bytes[at + 8 + 4 * i..at + 12 + 4 * i].copy_from_slice(&v.to_le_bytes());
    }
    bytes[at + 0x20..at + 0x24].copy_from_slice(b"SSbd");
    bytes[at + 0x24..at + 0x28].copy_from_slice(&(pcm.len() as u32).to_le_bytes());
    bytes[at + 0x28..at + 0x28 + pcm.len()].copy_from_slice(&pcm);
    let mut at = 2100 * SECTOR;
    for seed in 4..7 {
        let blocks = spu(8000, seed);
        bytes[at..at + 4].copy_from_slice(if headed { b"VAGp" } else { b"XAGp" });
        bytes[at + 0x0c..at + 0x10].copy_from_slice(&(blocks.len() as u32).to_be_bytes());
        bytes[at + 0x10..at + 0x14].copy_from_slice(&22_050u32.to_be_bytes());
        bytes[at + 0x30..at + 0x30 + blocks.len()].copy_from_slice(&blocks);
        at += (0x30 + blocks.len()).next_multiple_of(SECTOR);
    }
    std::fs::write(&iso, bytes).unwrap();
    iso
}

/// A DVD of 2,000 sectors in `dir`: its file system, a `SYSTEM.CNF` holding
/// `cnf`, noise, a long run of zeros and one block of noise twice.
fn disc(dir: &Path, cnf: &str) -> std::path::PathBuf {
    disc_with(dir, b"SYSTEM.CNF;1", cnf)
}

/// [`disc`], its one file named `name`.
fn disc_with(dir: &Path, name: &[u8], cnf: &str) -> std::path::PathBuf {
    let mut iso = vec![0; 2000 * SECTOR];
    let pvd = &mut iso[16 * SECTOR..17 * SECTOR];
    pvd[0] = 1;
    pvd[1..6].copy_from_slice(b"CD001");
    pvd[156..190].copy_from_slice(&record(&[0], 18, SECTOR as u32, true));
    let mut root = record(&[0], 18, SECTOR as u32, true);
    root.extend(record(&[1], 18, SECTOR as u32, true));
    root.extend(record(name, 19, cnf.len() as u32, false));
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

#[test]
fn a_ps2_dvd_codes_its_audio_as_audio_alone_and_in_a_store() {
    let mut sizes = Vec::new();
    for headed in [true, false] {
        let dir = TempDir::new().unwrap();
        let iso = audio_disc(dir.path(), headed);
        let ore = dir.path().join("game.ore");
        let stats = ore_rs::compress(&iso, &ore, &Options::default(), &[], &mut |_| {}).unwrap();
        sizes.push(stats.output_size);
        let back = dir.path().join("back.iso");
        ore_rs::decompress(&ore, &back, &mut |_| {}).unwrap();
        assert!(std::fs::read(&iso).unwrap() == std::fs::read(&back).unwrap());
        // Read across the audio areas, which lie out of disc order
        let mut reader = Reader::open(&ore).unwrap();
        let mut buf = vec![0; 3 * SECTOR];
        reader.read_at(649 * SECTOR as u64 + 7, &mut buf).unwrap();
        let disc = std::fs::read(&iso).unwrap();
        assert!(buf == disc[649 * SECTOR + 7..652 * SECTOR + 7]);
        if !headed {
            continue;
        }
        // Into a store and out of it again, each area's codec kept
        let store = dir.path().join(".ore-store");
        ore_rs::create_store(&store, &Default::default()).unwrap();
        let thin = dir.path().join("thin.ore");
        ore_rs::attach(&ore, &thin, &store, &Options::default(), &mut |_| {}).unwrap();
        let detached = dir.path().join("detached.ore");
        let stats = ore_rs::detach(&thin, &detached, &Options::default(), &mut |_| {}).unwrap();
        assert_eq!(stats.output_size, sizes[0]);
        ore_rs::decompress(&detached, &back, &mut |_| {}).unwrap();
        assert!(disc == std::fs::read(&back).unwrap());
    }
    // The audio coded as audio takes a tenth less than as bytes
    assert!(sizes[0] < sizes[1] * 9 / 10, "{sizes:?}");
}

#[test]
fn a_psp_umd_round_trips_as_a_ps2_dvd_does() {
    let dir = TempDir::new().unwrap();
    let iso = disc_with(
        dir.path(),
        b"UMD_DATA.BIN;1",
        "ULUS-10000|0000000000000001|0001|G",
    );
    let ore = dir.path().join("game.ore");
    let stats = ore_rs::compress(&iso, &ore, &Options::default(), &[], &mut |_| {}).unwrap();
    assert!(stats.output_size < 790 * SECTOR as u64);
    let back = dir.path().join("back.iso");
    ore_rs::decompress(&ore, &back, &mut |_| {}).unwrap();
    assert!(std::fs::read(&iso).unwrap() == std::fs::read(&back).unwrap());
}

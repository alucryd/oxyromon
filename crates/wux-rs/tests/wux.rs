//! WUD to WUX and back, on generated images.

mod common;

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use common::{SECTOR, noise};
use wux_rs::{Error, HEADER_SIZE, Image, MAGIC, SECTOR_SIZE};

/// Sectors of noise, repeats, zeros, and a short last sector.
fn image() -> Vec<u8> {
    let a = noise(SECTOR, 10);
    let b = noise(SECTOR, 11);
    let zero = vec![0; SECTOR];
    [&a, &zero, &b, &a, &zero, &zero, &b, &noise(1000, 12)[..]].concat()
}

fn round_trip(data: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    let (wud, wux, out) = (
        dir.path().join("g.wud"),
        dir.path().join("g.wux"),
        dir.path().join("o.wud"),
    );
    std::fs::write(&wud, data).unwrap();
    let mut read = 0;
    let stats = wux_rs::compress(&wud, &wux, &mut |n| read += n).unwrap();
    assert_eq!(read, data.len() as u64);
    assert_eq!(stats.output_size, std::fs::metadata(&wux).unwrap().len());
    let mut read = 0;
    wux_rs::decompress(&wux, &out, &mut |n| read += n).unwrap();
    assert_eq!(read, stats.output_size);
    (std::fs::read(&wux).unwrap(), std::fs::read(&out).unwrap())
}

#[test]
fn wud_to_wux_and_back_is_lossless() {
    let data = image();
    let (wux, wud) = round_trip(&data);
    assert_eq!(wud, data);

    // The header Cemu reads: magic, sector size, uncompressed size, flags.
    assert_eq!(wux[..8], MAGIC);
    assert_eq!(
        u32::from_le_bytes(wux[8..12].try_into().unwrap()),
        SECTOR_SIZE
    );
    assert_eq!(
        u64::from_le_bytes(wux[16..24].try_into().unwrap()),
        data.len() as u64
    );
    assert_eq!(wux[24..32], [0; 8]);
    // One table entry per sector, naming stored sectors in first-use order.
    let table: Vec<u32> = wux[32..32 + 8 * 4]
        .chunks(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    assert_eq!(table, [0, 1, 2, 0, 1, 1, 2, 3]);
    // Header and table padded to a sector, then the four distinct sectors.
    assert_eq!(wux.len(), SECTOR + 4 * SECTOR);
    assert_eq!(&wux[SECTOR..2 * SECTOR], &data[..SECTOR]);
}

#[test]
fn empty_and_single_sector_images_survive() {
    assert_eq!(round_trip(&[]).1, Vec::<u8>::new());
    let one = noise(SECTOR, 5);
    assert_eq!(round_trip(&one).1, one);
}

#[test]
fn an_image_reads_the_same_from_wud_and_wux() {
    let data = image();
    let dir = tempfile::tempdir().unwrap();
    let (wud, wux) = (dir.path().join("g.wud"), dir.path().join("g.wux"));
    std::fs::write(&wud, &data).unwrap();
    wux_rs::compress(&wud, &wux, &mut |_| {}).unwrap();
    for path in [&wud, &wux] {
        let mut image = Image::open(path).unwrap();
        assert_eq!(image.size(), data.len() as u64);
        // Across sector boundaries, and into the short last sector.
        let mut buf = vec![0; SECTOR + 100];
        image.read_exact_at(&mut buf, SECTOR as u64 - 50).unwrap();
        assert_eq!(buf, data[SECTOR - 50..2 * SECTOR + 50]);
        image.seek(SeekFrom::Start(0)).unwrap();
        let mut all = Vec::new();
        image.read_to_end(&mut all).unwrap();
        assert_eq!(all, data);
        assert!(
            image
                .read_exact_at(&mut [0; 2], data.len() as u64 - 1)
                .is_err()
        );
    }
    assert!(Image::open(&wud).unwrap().header().is_none());
    assert!(Image::open(&wux).unwrap().header().is_some());
}

#[test]
fn compressing_a_wux_or_decompressing_a_wud_fails() {
    let dir = tempfile::tempdir().unwrap();
    let (wud, wux, out) = (
        dir.path().join("g.wud"),
        dir.path().join("g.wux"),
        dir.path().join("o"),
    );
    std::fs::write(&wud, image()).unwrap();
    wux_rs::compress(&wud, &wux, &mut |_| {}).unwrap();
    assert!(matches!(
        wux_rs::compress(&wux, &out, &mut |_| {}),
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(
        wux_rs::decompress(&wud, &out, &mut |_| {}),
        Err(Error::BadMagic { .. })
    ));
    assert_no_output(&out);
}

#[test]
fn a_wux_naming_a_missing_sector_is_corrupt() {
    let dir = tempfile::tempdir().unwrap();
    let (wud, wux, out) = (
        dir.path().join("g.wud"),
        dir.path().join("g.wux"),
        dir.path().join("o"),
    );
    std::fs::write(&wud, image()).unwrap();
    wux_rs::compress(&wud, &wux, &mut |_| {}).unwrap();
    let mut bytes = std::fs::read(&wux).unwrap();
    let at = HEADER_SIZE as usize + 4;
    bytes[at..at + 4].copy_from_slice(&9u32.to_le_bytes());
    std::fs::write(&wux, bytes).unwrap();
    assert!(matches!(
        wux_rs::decompress(&wux, &out, &mut |_| {}),
        Err(Error::Corrupt(_))
    ));
    assert_no_output(&out);
}

/// Neither the output nor its `.part` exists.
fn assert_no_output(output: &Path) {
    assert!(!output.exists());
    assert!(!output.with_file_name("o.part").exists());
}

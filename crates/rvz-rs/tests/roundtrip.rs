//! Every disc comes back out of an RVZ byte for byte, whatever the method,
//! the chunk size, or what the RVZ was written from.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use rvz_rs::{Compression, Error, Options, Reader};
use tempfile::TempDir;

mod common;
use common::{COMMON_KEY, WII_PARTITION_DATA, gamecube_disc, wii_disc, write};

const METHODS: [(Compression, i32); 5] = [
    (Compression::None, 0),
    (Compression::Bzip2, 1),
    (Compression::Lzma, 1),
    (Compression::Lzma2, 1),
    (Compression::Zstd, 5),
];

fn options(compression: Compression, level: i32, chunk_size: u32) -> Options {
    Options {
        compression,
        level,
        chunk_size,
        scrub: false,
    }
}

/// Compress `disc`, check the RVZ decompresses back to it, and return the RVZ.
fn round_trip(dir: &Path, disc: &[u8], options: &Options, keys: &[[u8; 16]]) -> Vec<u8> {
    let iso = dir.join("disc.iso");
    let rvz = dir.join("disc.rvz");
    let out = dir.join("out.iso");
    write(&iso, disc);
    let mut consumed = 0;
    rvz_rs::compress(&iso, &rvz, options, keys, &mut |n| consumed += n).unwrap();
    assert_eq!(consumed, disc.len() as u64, "progress adds up to the input");
    let rvz_size = std::fs::metadata(&rvz).unwrap().len();
    let mut consumed = 0;
    rvz_rs::decompress(&rvz, &out, &mut |n| consumed += n).unwrap();
    assert_eq!(consumed, rvz_size, "progress adds up to the input");
    assert!(
        std::fs::read(&out).unwrap() == disc,
        "{options:?} does not round-trip"
    );
    std::fs::read(&rvz).unwrap()
}

#[test]
fn a_gamecube_disc_round_trips_with_every_method() {
    let dir = TempDir::new().unwrap();
    let disc = gamecube_disc(0x580000);
    for (compression, level) in METHODS {
        round_trip(
            dir.path(),
            &disc,
            &options(compression, level, 0x20000),
            &[],
        );
    }
}

#[test]
fn a_gamecube_disc_round_trips_with_every_chunk_size() {
    let dir = TempDir::new().unwrap();
    let disc = gamecube_disc(0x580000);
    for chunk_size in [0x8000, 0x20000, 0x200000, 0x400000] {
        round_trip(
            dir.path(),
            &disc,
            &options(Compression::Zstd, 3, chunk_size),
            &[],
        );
    }
}

#[test]
fn a_wii_disc_round_trips_with_every_method() {
    let dir = TempDir::new().unwrap();
    let disc = wii_disc();
    for (compression, level) in METHODS {
        round_trip(
            dir.path(),
            &disc,
            &options(compression, level, 0x20000),
            &[COMMON_KEY],
        );
    }
}

#[test]
fn a_wii_disc_round_trips_with_every_chunk_size() {
    let dir = TempDir::new().unwrap();
    let disc = wii_disc();
    for chunk_size in [0x8000, 0x20000, 0x200000, 0x400000] {
        round_trip(
            dir.path(),
            &disc,
            &options(Compression::Zstd, 3, chunk_size),
            &[COMMON_KEY],
        );
    }
}

#[test]
fn padding_is_stored_as_its_seed() {
    let dir = TempDir::new().unwrap();
    let disc = gamecube_disc(0x580000);
    let rvz = round_trip(
        dir.path(),
        &disc,
        &options(Compression::None, 0, 0x20000),
        &[],
    );
    // Junk fills most of the disc and takes 72 bytes a block once packed
    assert!(
        rvz.len() < disc.len() / 2,
        "{} of {}",
        rvz.len(),
        disc.len()
    );
}

#[test]
fn the_right_common_key_is_found_among_others() {
    let dir = TempDir::new().unwrap();
    round_trip(
        dir.path(),
        &wii_disc(),
        &Options::default(),
        &[[0x22; 16], COMMON_KEY, [0x33; 16]],
    );
}

#[test]
fn a_wii_disc_needs_its_common_key() {
    let dir = TempDir::new().unwrap();
    let iso = dir.path().join("disc.iso");
    let rvz = dir.path().join("disc.rvz");
    write(&iso, &wii_disc());
    for keys in [&[][..], &[[0x22; 16]][..]] {
        let error =
            rvz_rs::compress(&iso, &rvz, &Options::default(), keys, &mut |_| {}).unwrap_err();
        assert!(matches!(error, Error::MissingKey(_)), "{error}");
        assert!(!rvz.exists(), "a failed run leaves nothing behind");
    }
}

#[test]
fn an_rvz_is_rewritten_without_a_key() {
    let dir = TempDir::new().unwrap();
    let disc = wii_disc();
    let first = round_trip(dir.path(), &disc, &Options::default(), &[COMMON_KEY]);
    // Rewriting keeps the title keys the RVZ holds
    let rvz = dir.path().join("disc.rvz");
    let again = dir.path().join("again.rvz");
    rvz_rs::compress(&rvz, &again, &Options::default(), &[], &mut |_| {}).unwrap();
    assert!(
        std::fs::read(&again).unwrap() == first,
        "rewriting changes nothing"
    );
    let other = dir.path().join("other.rvz");
    let out = dir.path().join("other.iso");
    rvz_rs::compress(
        &rvz,
        &other,
        &options(Compression::Bzip2, 9, 0x200000),
        &[],
        &mut |_| {},
    )
    .unwrap();
    rvz_rs::decompress(&other, &out, &mut |_| {}).unwrap();
    assert!(std::fs::read(&out).unwrap() == disc);
}

/// Scrub `disc`, returning what the scrubbed RVZ decompresses to.
fn scrubbed(dir: &Path, disc: &[u8], keys: &[[u8; 16]]) -> Vec<u8> {
    let iso = dir.join("disc.iso");
    let rvz = dir.join("scrubbed.rvz");
    let out = dir.join("scrubbed.iso");
    write(&iso, disc);
    let options = Options {
        scrub: true,
        ..Options::default()
    };
    rvz_rs::compress(&iso, &rvz, &options, keys, &mut |_| {}).unwrap();
    rvz_rs::decompress(&rvz, &out, &mut |_| {}).unwrap();
    std::fs::read(out).unwrap()
}

#[test]
fn scrubbing_a_gamecube_disc_keeps_what_it_points_to() {
    let dir = TempDir::new().unwrap();
    let disc = gamecube_disc(0x580000);
    let out = scrubbed(dir.path(), &disc, &[]);
    assert_eq!(out.len(), disc.len());
    // Header, apploader, DOL, file system table and files are kept
    for (start, end) in [
        (0, 0x3440),
        (0x10000, 0x12100),
        (0x20000, 0x20060),
        (0x30000, 0x42345),
        (0x100000, 0x140000),
    ] {
        assert!(
            out[start..end] == disc[start..end],
            "{start:#x}..{end:#x} changed"
        );
    }
    // Clusters nothing points to are zeroed
    assert!(out[0x200000..].iter().all(|&byte| byte == 0));
    assert!(disc[0x200000..].iter().any(|&byte| byte != 0));
}

#[test]
fn scrubbing_a_wii_disc_keeps_its_partition() {
    let dir = TempDir::new().unwrap();
    let disc = wii_disc();
    let out = scrubbed(dir.path(), &disc, &[COMMON_KEY]);
    assert_eq!(out.len(), disc.len());
    // The partition's header, and its first block, holding its boot area,
    // are kept, but its second block holds nothing but junk
    assert!(out[..WII_PARTITION_DATA + 0x8000] == disc[..WII_PARTITION_DATA + 0x8000]);
    let second = WII_PARTITION_DATA + 0x8000..WII_PARTITION_DATA + 0x10000;
    assert!(out[second].iter().all(|&byte| byte == 0));
    // Past the partition, nothing is pointed to
    let end = WII_PARTITION_DATA + common::WII_BLOCKS * 0x8000;
    assert!(out[end..].iter().all(|&byte| byte == 0));
}

#[test]
fn a_reader_reads_and_seeks_anywhere() {
    let dir = TempDir::new().unwrap();
    let disc = wii_disc();
    round_trip(dir.path(), &disc, &Options::default(), &[COMMON_KEY]);
    let mut reader = Reader::open(&dir.path().join("disc.rvz")).unwrap();
    assert_eq!(reader.size(), disc.len() as u64);
    for (offset, length) in [
        (0, 0x10),
        (0x7F, 0x2),
        (0x4FFF0, 0x40),
        (WII_PARTITION_DATA as u64 + 0x1FFFF0, 0x40),
        (disc.len() as u64 - 3, 3),
    ] {
        let mut buf = vec![0; length];
        reader.seek(SeekFrom::Start(offset)).unwrap();
        reader.read_exact(&mut buf).unwrap();
        assert!(
            buf == disc[offset as usize..offset as usize + length],
            "at {offset:#x}"
        );
    }
    let mut rest = Vec::new();
    reader.seek(SeekFrom::End(-0x1000)).unwrap();
    reader.read_to_end(&mut rest).unwrap();
    assert!(rest == disc[disc.len() - 0x1000..]);
}

#[test]
fn refuses_what_is_not_an_rvz() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("not.rvz");
    let mut wia = vec![0; 0x100];
    wia[..4].copy_from_slice(b"WIA\x01");
    write(&path, &wia);
    assert!(matches!(Reader::open(&path), Err(Error::Unsupported(_))));
    write(&path, &[0; 0x100]);
    assert!(matches!(Reader::open(&path), Err(Error::BadMagic { .. })));
}

#[test]
fn refuses_a_bad_chunk_size() {
    let dir = TempDir::new().unwrap();
    let iso = dir.path().join("disc.iso");
    write(&iso, &gamecube_disc(0x580000));
    for chunk_size in [0x4000, 0x30000, 0x300000] {
        let error = rvz_rs::compress(
            &iso,
            &dir.path().join("x.rvz"),
            &options(Compression::Zstd, 5, chunk_size),
            &[],
            &mut |_| {},
        )
        .unwrap_err();
        assert!(
            matches!(error, Error::InvalidOption(_)),
            "{chunk_size:#x}: {error}"
        );
    }
}

/// Rewrite both header hashes, so a change to the second header is only
/// caught by what checks its fields.
fn rehash(rvz: &mut [u8]) {
    use sha1::{Digest, Sha1};
    let header_2_size = u32::from_be_bytes(rvz[12..16].try_into().unwrap()) as usize;
    let hash = Sha1::digest(&rvz[0x48..0x48 + header_2_size]);
    rvz[16..36].copy_from_slice(&hash);
    let hash = Sha1::digest(&rvz[..52]);
    rvz[52..72].copy_from_slice(&hash);
}

#[test]
fn refuses_a_damaged_rvz() {
    let dir = TempDir::new().unwrap();
    let damaged = dir.path().join("damaged.rvz");
    let back = dir.path().join("back.iso");
    let good = round_trip(
        dir.path(),
        &gamecube_disc(0x580000),
        &options(Compression::Zstd, 5, 0x20000),
        &[],
    );
    let refused = |bytes: &[u8]| {
        write(&damaged, bytes);
        let error = rvz_rs::decompress(&damaged, &back, &mut |_| {}).unwrap_err();
        assert!(matches!(error, Error::Corrupt(_)), "{error}");
        assert!(!back.exists());
    };

    // Cut short
    refused(&good[..good.len() - 0x200]);

    // Either header flipped
    for offset in [0x20, 0x48 + 0x20] {
        let mut bad = good.clone();
        bad[offset] ^= 0xFF;
        refused(&bad);
    }

    // A chunk size no RVZ has, or one too large to allocate, behind valid hashes
    for chunk_size in [0u32, 0x30000, 0x8000_0000] {
        let mut bad = good.clone();
        bad[0x48 + 12..0x48 + 16].copy_from_slice(&chunk_size.to_be_bytes());
        rehash(&mut bad);
        refused(&bad);
    }
}

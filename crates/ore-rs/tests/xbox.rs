//! An Xbox disc as Redump dumps one, XGD1, is recognised by its game
//! partition's volume descriptor and comes back byte for byte: its video
//! partition, its files and the filler between them, noise no codec
//! shrinks, unless an early layout tool's generator made it, which the
//! ORE regenerates.

// They write the OREs they read
#![cfg(feature = "write")]

use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use ore_rs::{Options, Reader};
use tempfile::TempDir;

const SECTOR: u64 = 2048;
const GAME: u64 = 0x1830_0000;
const MAGIC: &[u8; 20] = b"MICROSOFT*XBOX*MEDIA";

/// Noise, seeded, as an XGD1 disc's filler is.
fn noise(seed: u64, len: usize) -> Vec<u8> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 56) as u8
        })
        .collect()
}

/// A disc of XGD1's layout, sparse: its video partition zeros but a
/// volume descriptor, then its game partition's, whose tail repeats the
/// magic unless `tail` is false, a file's worth of text, and filler.
fn disc(dir: &Path, tail: bool) -> PathBuf {
    let path = dir.join("Game (USA).iso");
    let mut file = File::create(&path).unwrap();
    file.seek(SeekFrom::Start(16 * SECTOR)).unwrap();
    file.write_all(b"\x01CD001\x01").unwrap();
    let mut descriptor = vec![0; SECTOR as usize];
    descriptor[..20].copy_from_slice(MAGIC);
    if tail {
        descriptor[SECTOR as usize - 20..].copy_from_slice(MAGIC);
    }
    file.seek(SeekFrom::Start(GAME + 32 * SECTOR)).unwrap();
    file.write_all(&descriptor).unwrap();
    let text: Vec<u8> = (0..3 << 20).map(|i| b"default.xbe "[i % 12]).collect();
    file.write_all(&text).unwrap();
    file.write_all(&noise(7, 3 << 20)).unwrap();
    file.write_all(&vec![0; 64 * SECTOR as usize]).unwrap();
    drop(file);
    path
}

/// The early layout tools' filler, from xboxdevwiki's XDVDFS page: a
/// state stepped modulo a prime, two bytes a step.
struct Filler(u32, u32, u32);

impl Filler {
    fn new(seed: u32) -> Self {
        const B: [u32; 8] = [
            0x52F690D5, 0x534D7DDE, 0x5B71A70F, 0x66793320, 0x9B7E5ED5, 0xA465265E, 0xA53F1D11,
            0xB154430F,
        ];
        let mut filler = Self(0, B[seed as usize & 7], seed);
        filler.0 = filler.step();
        filler
    }

    fn step(&mut self) -> u32 {
        self.2 = ((self.2 as u64 + 1) * self.1 as u64 % 0xFFFFFFFB) as u32;
        self.2 ^ self.0
    }

    fn sectors(&mut self, count: usize) -> Vec<u8> {
        (0..count * SECTOR as usize / 2)
            .flat_map(|_| ((self.step() >> 8) as u16).to_le_bytes())
            .collect()
    }
}

/// A disc an early layout tool mastered: its game partition's reserved
/// sectors filler, its descriptor and the tool's signature, a file, a
/// security range the stream runs on through, and filler to its end.
fn seeded_disc(dir: &Path) -> PathBuf {
    let path = dir.join("Early (USA).iso");
    let mut file = File::create(&path).unwrap();
    file.seek(SeekFrom::Start(16 * SECTOR)).unwrap();
    file.write_all(b"\x01CD001\x01").unwrap();
    let mut filler = Filler::new(5);
    file.seek(SeekFrom::Start(GAME)).unwrap();
    file.write_all(&filler.sectors(32)).unwrap();
    let mut descriptor = vec![0; SECTOR as usize];
    descriptor[..20].copy_from_slice(MAGIC);
    descriptor[SECTOR as usize - 20..].copy_from_slice(MAGIC);
    file.write_all(&descriptor).unwrap();
    let mut layout = vec![0x11; SECTOR as usize];
    layout[..24].copy_from_slice(b"XBOX_DVD_LAYOUT_TOOL_SIG");
    layout[0x24..0x26].copy_from_slice(&4134u16.to_le_bytes());
    file.write_all(&layout).unwrap();
    let text: Vec<u8> = (0..1 << 20).map(|i| b"default.xbe "[i % 12]).collect();
    file.write_all(&text).unwrap();
    file.write_all(&noise(9, 16 * SECTOR as usize)).unwrap();
    filler.sectors(16);
    file.write_all(&filler.sectors(3 << 9)).unwrap();
    drop(file);
    path
}

#[test]
fn an_early_xbox_discs_filler_is_regenerated() {
    let dir = TempDir::new().unwrap();
    let iso = seeded_disc(dir.path());
    let ore = dir.path().join("Early (USA).ore");
    let stats = ore_rs::compress(&iso, &ore, &Options::default(), &[], &mut |_| {}).unwrap();
    // The filler costs its records, the security range its noise
    assert!(stats.output_size < 1 << 20, "{}", stats.output_size);

    let bytes = std::fs::read(&iso).unwrap();
    let mut reader = Reader::open(&ore).unwrap();
    assert_eq!(reader.platform(), "xbox");
    // Within a record, from an odd byte
    let at = GAME as usize + 34 * SECTOR as usize + (1 << 20) + 17 * SECTOR as usize + 333;
    let mut buf = vec![0; 5000];
    reader.read_at(at as u64, &mut buf).unwrap();
    assert_eq!(buf, bytes[at..at + 5000]);

    let back = dir.path().join("back.iso");
    ore_rs::decompress(&ore, &back, &mut |_| {}).unwrap();
    assert_eq!(std::fs::read(back).unwrap(), bytes);
}

#[test]
fn an_xbox_disc_comes_back_byte_for_byte() {
    let dir = TempDir::new().unwrap();
    let iso = disc(dir.path(), true);
    let ore = dir.path().join("Game (USA).ore");
    let stats = ore_rs::compress(&iso, &ore, &Options::default(), &[], &mut |_| {}).unwrap();
    let reader = Reader::open(&ore).unwrap();
    assert_eq!(reader.platform(), "xbox");
    // The zeros and the text cost next to nothing, the filler its size
    assert!(stats.output_size > 3 << 20, "{}", stats.output_size);
    assert!(stats.output_size < 4 << 20, "{}", stats.output_size);

    let back = dir.path().join("back.iso");
    ore_rs::decompress(&ore, &back, &mut |_| {}).unwrap();
    assert_eq!(std::fs::read(back).unwrap(), std::fs::read(iso).unwrap());
}

#[test]
fn a_disc_whose_descriptor_lacks_its_tail_is_no_xbox_disc() {
    let dir = TempDir::new().unwrap();
    let iso = disc(dir.path(), false);
    let ore = dir.path().join("Game (USA).ore");
    let error = ore_rs::compress(&iso, &ore, &Options::default(), &[], &mut |_| {}).unwrap_err();
    assert!(matches!(error, ore_rs::Error::Unsupported(_)), "{error}");
    assert!(!ore.exists());
}

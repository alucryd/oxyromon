//! The conformance vectors of SPEC.md §16: every vector reads to the logical
//! stream and the files its manifest gives, every hostile one is refused.
//! `write_vectors`, ignored, makes them again from synthetic inputs:
//! `cargo test -p ore-rs --test vectors -- --ignored write_vectors`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ore_rs::Reader;
use sha1::{Digest, Sha1};

// The padding generator as the library has it, to check the spec's text by
#[allow(dead_code)]
#[path = "../../gcwii/src/lfg.rs"]
mod lfg;

fn vectors() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("spec/vectors")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A vector of the manifest: whether it reads, its logical stream's size and
/// BLAKE3, and its output files, each a path, a size and a SHA-1.
struct Vector {
    ok: bool,
    logical: Option<(u64, String)>,
    files: Vec<(String, u64, String)>,
}

fn manifest() -> BTreeMap<String, Vector> {
    let text = std::fs::read_to_string(vectors().join("manifest.tsv")).unwrap();
    let mut vectors: BTreeMap<String, Vector> = BTreeMap::new();
    for line in text.lines().filter(|line| !line.starts_with('#')) {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields[..] {
            ["vector", path, "ok", size, blake3, _] => {
                let logical = Some((size.parse().unwrap(), blake3.into()));
                vectors.insert(
                    path.into(),
                    Vector {
                        ok: true,
                        logical,
                        files: Vec::new(),
                    },
                );
            }
            ["vector", path, "refuse", _, _, _] => {
                vectors.insert(
                    path.into(),
                    Vector {
                        ok: false,
                        logical: None,
                        files: Vec::new(),
                    },
                );
            }
            ["file", path, name, size, sha1] => vectors.get_mut(path).unwrap().files.push((
                name.into(),
                size.parse().unwrap(),
                sha1.into(),
            )),
            _ => panic!("a manifest line: {line:?}"),
        }
    }
    vectors
}

/// Read `path` whole, then write it out under `files`' names in `out`: its
/// logical stream's size and BLAKE3.
fn read(path: &Path, files: &[(String, u64, String)], out: &Path) -> ore_rs::Result<(u64, String)> {
    let mut reader = Reader::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0; 1 << 20];
    let mut at = 0;
    while at < reader.size() {
        let length = buf.len().min((reader.size() - at) as usize);
        reader.read_at(at, &mut buf[..length])?;
        hasher.update(&buf[..length]);
        at += length as u64;
    }
    // The output is the title's folder, the CD's cue sheet, or the image
    let name = |pick: &dyn Fn(&str) -> bool| {
        files
            .iter()
            .map(|(name, _, _)| name.as_str())
            .find(|name| pick(name))
            .unwrap_or("out")
            .to_owned()
    };
    let output = if reader.is_title() {
        name(&|_| true).split('/').next().unwrap().to_owned()
    } else if reader.is_cd() {
        name(&|name| name.ends_with(".cue"))
    } else {
        name(&|_| true)
    };
    ore_rs::decompress(path, &out.join(output), &mut |_| {})?;
    Ok((reader.size(), hex(hasher.finalize().as_bytes())))
}

/// What ore-rs refuses each hostile vector with, so each is refused for
/// the reason the manifest names: other readers word theirs their own way.
const REFUSALS: [(&str, &str); 19] = [
    ("area-kind.ore", "unknown variant `rax`"),
    (
        "dedup-forward.ore",
        "a reference to a chunk not stored before it",
    ),
    ("envelope.ore", "the envelope of the group"),
    ("group-crc.ore", "Data corruption detected"),
    ("index-crc.ore", "index fails its CRC"),
    ("index-tail.ore", "the index holds more than it describes"),
    ("layout-2.ore", "an ORE of layout 2"),
    (
        "logical-hash.ore",
        "does not match the hash it was written with",
    ),
    ("magic.ore", "expected ORE1, found RIFF"),
    ("media-class.ore", "media class 255"),
    ("meta-crc.ore", "metadata fails its CRC"),
    ("objects.ore", "files of several objects"),
    ("record-kind.ore", "record kind 3"),
    ("record-len.ore", "a record of a bad length"),
    ("required-flag.ore", "required flags 0x80000000"),
    ("short-header.ore", "header of 128 bytes"),
    (
        "size.ore",
        "metadata and header disagree on the disc's size",
    ),
    ("store-record.ore", "record kind 9"),
    ("truncated.ore", "metadata lies past the end of the file"),
];

#[test]
fn vectors_read_as_their_manifest_says() {
    let vectors_dir = vectors();
    let manifest = manifest();
    assert!(manifest.len() > 20, "{} vectors", manifest.len());
    for (path, vector) in &manifest {
        // A vector gone would refuse as one that is not an ORE
        assert!(vectors_dir.join(path).is_file(), "{path} is missing");
        let out = tempfile::TempDir::new().unwrap();
        let result = read(&vectors_dir.join(path), &vector.files, out.path());
        if !vector.ok {
            let error = match result {
                Ok(_) => panic!("{path} is read, not refused"),
                Err(e) => e.to_string(),
            };
            let (_, why) = REFUSALS
                .iter()
                .find(|(refused, _)| path.ends_with(refused))
                .unwrap_or_else(|| panic!("{path}: no refusal expected of ore-rs"));
            assert!(error.contains(why), "{path} is refused for {error:?}");
            continue;
        }
        let logical = result.unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(Some(&logical), vector.logical.as_ref(), "{path}");
        for (name, size, sha1) in &vector.files {
            let bytes = std::fs::read(out.path().join(name)).unwrap();
            assert_eq!(bytes.len() as u64, *size, "{path}: {name}");
            assert_eq!(&hex(&Sha1::digest(&bytes)), sha1, "{path}: {name}");
        }
    }
}

/// SPEC.md's Appendix A, implemented from its text alone: what it says the
/// padding generator outputs is what GameCube and Wii discs hold.
#[test]
fn the_specs_padding_generator_is_the_discs() {
    fn spec(seed: &[u8; 68], len: usize) -> Vec<u8> {
        let mut b = [0u32; 521];
        for (i, word) in seed.chunks(4).enumerate() {
            b[i] = u32::from_be_bytes(word.try_into().unwrap());
        }
        for i in 17..521 {
            b[i] = (b[i - 17] << 23) ^ (b[i - 16] >> 9) ^ b[i - 1];
        }
        for word in &mut b {
            *word = (*word & 0xFF00FFFF) | ((*word >> 2) & 0x00FF0000);
        }
        let step = |b: &mut [u32; 521]| {
            for i in 0..32 {
                b[i] ^= b[i + 489];
            }
            for i in 32..521 {
                b[i] ^= b[i - 32];
            }
        };
        for _ in 0..4 {
            step(&mut b);
        }
        let mut out = Vec::new();
        while out.len() < len {
            out.extend(b.iter().flat_map(|word| word.to_be_bytes()));
            step(&mut b);
        }
        out.truncate(len);
        out
    }
    let seed: [u8; 68] = std::array::from_fn(|i| (i as u8).wrapping_mul(37).wrapping_add(11));
    let mut lfg = lfg::Lfg::from_bytes(&seed);
    let mut disc = vec![0; 3 * 2084 + 100];
    lfg.fill(&mut disc);
    assert!(spec(&seed, disc.len()) == disc);
}

// What the inputs are built with
#[cfg(feature = "write")]
#[allow(dead_code)]
#[path = "../../cd/src/lib.rs"]
mod cd;
#[cfg(feature = "write")]
#[path = "../../gcwii/tests/common/mod.rs"]
mod gcwii;
#[cfg(feature = "write")]
#[path = "../../wiiu/tests/common/mod.rs"]
mod wiiu;

#[cfg(feature = "write")]
mod write {
    use super::*;

    use ore_rs::{Codec, Options, Profile, StoreOptions};

    use super::cd::sector::{self, Kind, LEAD_IN, SECTOR};
    use super::{gcwii, wiiu};

    /// The manifest being written, and the vectors' directory.
    struct Vectors {
        dir: PathBuf,
        lines: Vec<String>,
    }

    impl Vectors {
        /// `path`, which reads as `logical` and writes out as `files`.
        fn ok(&mut self, path: &str, covers: &str, logical: &[u8], files: &[(String, Vec<u8>)]) {
            self.lines.push(format!(
                "vector\t{path}\tok\t{}\t{}\t{covers}",
                logical.len(),
                hex(blake3::hash(logical).as_bytes())
            ));
            for (name, bytes) in files {
                self.lines.push(format!(
                    "file\t{path}\t{name}\t{}\t{}",
                    bytes.len(),
                    hex(&Sha1::digest(bytes))
                ));
            }
        }

        fn refuse(&mut self, path: &str, why: &str) {
            self.lines
                .push(format!("vector\t{path}\trefuse\t-\t-\t{why}"));
        }

        /// Compress `input` to the vector `path`.
        fn compress(&self, input: &Path, path: &str, options: &Options, keys: &[[u8; 16]]) {
            let ore = self.dir.join(path);
            std::fs::create_dir_all(ore.parent().unwrap()).unwrap();
            ore_rs::compress(input, &ore, options, keys, &mut |_| {}).unwrap();
        }
    }

    /// Text-like bytes from a few words: chunks that compress, all different.
    fn text(len: usize, mut state: u64) -> Vec<u8> {
        const WORDS: [&str; 12] = [
            "ore ", "disc ", "track ", "sector ", "chunk ", "group ", "store ", "area ", "record ",
            "index ", "codec ", "seed ",
        ];
        let mut out = Vec::with_capacity(len + 8);
        while out.len() < len {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            out.extend_from_slice(WORDS[(state >> 60) as usize % WORDS.len()].as_bytes());
        }
        out.truncate(len);
        out
    }

    /// A cartridge's ROM of 512 KiB: text, zeros, the same text again,
    /// zeros, then `0xFF` to its end: chunks, the copy's `DEDUP` records, as
    /// the zeros end the chunks before it, `ZERO`s and a `fill`.
    fn cartridge(seed: u64) -> Vec<u8> {
        let block = text(96 << 10, seed);
        let mut rom = [block.clone(), vec![0; 32 << 10], block].concat();
        rom.resize(256 << 10, 0);
        rom.resize(512 << 10, 0xFF);
        rom
    }

    /// Small noise, as a recording's: what keeps zstd from out-coding the
    /// audio models on a pure tone.
    fn dither(n: usize) -> i32 {
        let x = (n as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        ((x >> 59) as i32) - 16
    }

    /// A sector of stereo XA ADPCM of a noisy tone from sample `first`,
    /// each unit's shift the finest whose steps reach its residuals, as an
    /// encoder picks it: what the `xa` model codes smaller than zstd.
    fn xa_sector(first: usize) -> Vec<u8> {
        let x = |n: usize| ((n as f64 * 0.05).sin() * 8000.0) as i32 + dither(n) * 64;
        let mut sector = vec![0u8; 2324];
        let (mut old, mut n) = ([0i32; 2], first);
        for group in sector[..18 * 128].as_chunks_mut::<128>().0 {
            for unit in 0..8 {
                let old = &mut old[unit & 1];
                // Filter 1: 60/64 of the last sample
                let fit = (0..=12u32).rev().find_map(|shift| {
                    let (mut o, mut nibbles) = (*old, [0i32; 28]);
                    for (j, nibble) in nibbles.iter_mut().enumerate() {
                        let pred = (o * 60 + 32) >> 6;
                        *nibble = (x(n + j) - pred) >> (12 - shift);
                        if !(-8..=7).contains(nibble) {
                            return None;
                        }
                        o = (((*nibble << 12) >> shift) + pred).clamp(-32768, 32767);
                    }
                    Some((shift, nibbles, o))
                });
                let (shift, nibbles, last) = fit.expect("shift 0 reaches any residual");
                *old = last;
                group[4 + unit] = (1 << 4 | shift) as u8;
                for (j, nibble) in nibbles.iter().enumerate() {
                    group[16 + j * 4 + unit / 2] |= ((nibble & 15) as u8) << ((unit & 1) * 4);
                }
                n += 28;
            }
            group.copy_within(4..8, 0);
            group.copy_within(8..12, 12);
        }
        sector
    }

    fn image(name: &str, bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        vec![(name.into(), bytes.to_vec())]
    }

    /// A video sector of a still picture, 16x16, 4 sectors a frame, as
    /// tests/cd.rs makes them.
    fn still(frame: u32, chunk: u16) -> Vec<u8> {
        let mut data = vec![0; 2048];
        data[..4].copy_from_slice(&0x8001_0160u32.to_le_bytes());
        data[4..6].copy_from_slice(&chunk.to_le_bytes());
        data[6] = 4;
        data[8..12].copy_from_slice(&frame.to_le_bytes());
        data[12] = 20;
        data[16] = 16;
        data[18] = 16;
        if chunk == 0 {
            data[32..40].copy_from_slice(&[6, 0, 0, 0x38, 1, 0, 2, 0]);
            let bits = format!("{}0111111111", "000000000010".repeat(6));
            let bits = format!("{bits:0<96}");
            for (i, word) in bits.as_bytes().chunks(16).enumerate() {
                let word = u16::from_str_radix(std::str::from_utf8(word).unwrap(), 2).unwrap();
                data[40 + i * 2..42 + i * 2].copy_from_slice(&word.to_le_bytes());
            }
        }
        data
    }

    /// A block of an MDEC bitstream: its DC, then AC codes, each a run, a
    /// level and whether it is written as the escape.
    type Block = (i32, Vec<(u32, i32, bool)>);

    /// Bits, high first, then as the bitstream's 16-bit little-endian words.
    #[derive(Default)]
    struct Bits(Vec<bool>);

    impl Bits {
        fn put(&mut self, value: u32, n: u32) {
            self.0.extend((0..n).rev().map(|i| value >> i & 1 == 1));
        }

        fn words(&self) -> Vec<u8> {
            let mut bytes: Vec<u8> = self
                .0
                .chunks(8)
                .map(|b| b.iter().fold(0u8, |a, &bit| a << 1 | bit as u8) << (8 - b.len()))
                .collect();
            if bytes.len() % 2 == 1 {
                bytes.push(0);
            }
            bytes.chunks(2).flat_map(|w| [w[1], w[0]]).collect()
        }
    }

    /// A v3 frame of 2 macroblocks, 32x16, as one standard video sector:
    /// its DCs as MPEG-1's differences, its AC codes from the table and
    /// escaped, `frame` its number.
    fn mdec_frame(frame: u32, blocks: &[Block]) -> Vec<u8> {
        const CHROMA: [(u32, u32); 9] = [
            (0b00, 2),
            (0b01, 2),
            (0b10, 2),
            (0b110, 3),
            (0b1110, 4),
            (0b11110, 5),
            (0b111110, 6),
            (0b1111110, 7),
            (0b11111110, 8),
        ];
        const LUMA: [(u32, u32); 9] = [
            (0b100, 3),
            (0b00, 2),
            (0b01, 2),
            (0b101, 3),
            (0b110, 3),
            (0b1110, 4),
            (0b11110, 5),
            (0b111110, 6),
            (0b1111110, 7),
        ];
        // Some of MPEG-1's dct_coeff_next codes: run, level, code, length
        const TABLE: [(u32, i32, u32, u32); 4] = [
            (0, 1, 0b11, 2),
            (1, 1, 0b011, 3),
            (0, 2, 0b0100, 4),
            (2, 1, 0b0101, 4),
        ];
        let mut bits = Bits::default();
        let mut previous = [0i32; 3];
        for (b, (dc, acs)) in blocks.iter().enumerate() {
            let kind = [0, 1, 2, 2, 2, 2][b % 6];
            let diff = dc - previous[kind];
            previous[kind] = *dc;
            let size = 32 - diff.unsigned_abs().leading_zeros();
            let (code, n) = if b % 6 < 2 { CHROMA } else { LUMA }[size as usize];
            bits.put(code, n);
            if size > 0 {
                let v = if diff < 0 {
                    diff + (1 << size) - 1
                } else {
                    diff
                };
                bits.put(v as u32, size);
            }
            for &(run, level, escaped) in acs {
                match TABLE.iter().find(|t| t.0 == run && t.1 == level.abs()) {
                    Some(&(_, _, code, n)) if !escaped => {
                        bits.put(code, n);
                        bits.put((level < 0) as u32, 1);
                    }
                    _ => {
                        bits.put(0b000001, 6);
                        bits.put(run, 6);
                        bits.put(level as u32 & 0x3FF, 10);
                    }
                }
            }
            bits.put(0b10, 2);
        }
        // The BS header: its codes' count, 0x3800, the quantiser, v3
        let mut bs = vec![0x40, 0x01, 0x00, 0x38, 0x02, 0x00, 0x03, 0x00];
        bs.extend(bits.words());
        let mut data = vec![0; 2048];
        data[..4].copy_from_slice(&0x8001_0160u32.to_le_bytes());
        data[6] = 1;
        data[8..12].copy_from_slice(&frame.to_le_bytes());
        data[12..16].copy_from_slice(&(bs.len() as u32).to_le_bytes());
        data[16] = 32;
        data[18] = 16;
        data[32..32 + bs.len()].copy_from_slice(&bs);
        data
    }

    /// Two v3 frames, the second's even blocks its first's: every code of
    /// the bitstream, and blocks the same as the frame before's.
    fn mdec_frames() -> [Vec<u8>; 2] {
        let first: Vec<Block> = (0..12)
            .map(|b: i32| {
                let acs = [
                    (0, if b % 2 == 0 { 1 } else { -2 }, false),
                    (1, 1, b == 3),
                    (2, -1, false),
                    (5, 100, false),
                    (0, -37, false),
                ];
                ((b * 13) % 61 - 30, acs[..(b as usize % 5 + 1)].to_vec())
            })
            .collect();
        let second: Vec<Block> = first
            .iter()
            .enumerate()
            .map(|(b, block)| match b % 2 {
                0 => block.clone(),
                _ => (block.0 + 7, vec![(3, 2, false), (0, 1, true)]),
            })
            .collect();
        [mdec_frame(2, &first), mdec_frame(3, &second)]
    }

    fn sectors(first: u32, kinds: &[(Kind, [u8; 8], Vec<u8>)]) -> Vec<u8> {
        let mut track = Vec::new();
        for (i, (kind, subheader, data)) in kinds.iter().enumerate() {
            let mut raw = [0; SECTOR];
            sector::rebuild(*kind, first + i as u32, subheader, data, &mut raw);
            track.extend_from_slice(&raw);
        }
        track
    }

    /// A Mode 2 track of every kind of sector and plane: Form 1 data, XA
    /// ADPCM, Form 2 without an EDC, Form 2 data, subheader copies that
    /// differ, a still picture as STR video, a zero sector and a raw one.
    fn mode2_track(frame: u32) -> Vec<u8> {
        let form1 = [0, 0, 8, 0, 0, 0, 8, 0];
        let mut kinds = Vec::new();
        for i in 0..8 {
            kinds.push((Kind::Form1, form1, text(2048, i)));
        }
        // Enough of it for the model to pay: 4,032 samples a sector
        for i in 0..15 {
            kinds.push((
                Kind::Form2,
                [1, 1, 0x64, 1, 1, 1, 0x64, 1],
                xa_sector(i * 4032),
            ));
        }
        kinds.push((
            Kind::Form2NoEdc,
            [1, 2, 0x64, 1, 1, 2, 0x64, 1],
            xa_sector(15 * 4032),
        ));
        kinds.push((Kind::Form2, [0, 0, 0x20, 0, 0, 0, 0x20, 0], text(2324, 21)));
        kinds.push((
            Kind::Form2NoEdc,
            [1, 3, 0x64, 1, 1, 1, 0x64, 1],
            xa_sector(16 * 4032),
        ));
        for i in 0..8u32 {
            kinds.push((
                Kind::Form1,
                [1, 1, 0x48, 0, 1, 1, 0x48, 0],
                still(i / 4, (i % 4) as u16),
            ));
        }
        for data in mdec_frames() {
            kinds.push((Kind::Form1, [1, 1, 0x48, 0, 1, 1, 0x48, 0], data));
        }
        let mut track = sectors(frame, &kinds);
        track.extend_from_slice(&[0; SECTOR]);
        let mut raw = [0; SECTOR];
        sector::rebuild(
            Kind::Form1,
            frame + kinds.len() as u32 + 1,
            &form1,
            &text(2048, 30),
            &mut raw,
        );
        raw[0x900] ^= 0x5A;
        track.extend_from_slice(&raw);
        track
    }

    fn mode1_track(sectors: u32, frame: u32, seed: u64) -> Vec<u8> {
        let mut track = Vec::new();
        for i in 0..sectors {
            let mut raw = [0; SECTOR];
            sector::rebuild(
                Kind::Mode1,
                frame + i,
                &[0; 8],
                &text(2048, seed + i as u64),
                &mut raw,
            );
            track.extend_from_slice(&raw);
        }
        track
    }

    /// An audio track: a tone, then silence.
    fn audio_track(sectors: usize) -> Vec<u8> {
        let mut pcm = Vec::with_capacity(sectors * SECTOR);
        for i in 0..sectors * 588 {
            let sample = if i < sectors * 588 * 3 / 4 {
                ((i as f64 * 0.05).sin() * 8000.0) as i16
            } else {
                0
            };
            pcm.extend_from_slice(&sample.to_le_bytes());
            pcm.extend_from_slice(&(sample / 2).to_le_bytes());
        }
        pcm
    }

    /// A disc's cue sheet, its logical stream, and the files a reader
    /// writes back, each a name and its bytes.
    type CueDisc = (PathBuf, Vec<u8>, Vec<(String, Vec<u8>)>);

    /// A CD's or GD-ROM's tracks, written beside their cue sheet in `dir`:
    /// the cue sheet's path, and the files a reader writes back.
    fn cue_disc(dir: &Path, name: &str, tracks: &[(&str, Vec<u8>, &str)], sbi: bool) -> CueDisc {
        let mut cue = String::new();
        let mut files = Vec::new();
        let mut logical = Vec::new();
        for (i, (mode, track, before)) in tracks.iter().enumerate() {
            cue += before;
            let file = format!("{name} (Track {}).bin", i + 1);
            std::fs::write(dir.join(&file), track).unwrap();
            cue += &format!("FILE \"{file}\" BINARY\r\n  TRACK {:02} {mode}\r\n", i + 1);
            cue += if *mode == "AUDIO" && !before.contains("HIGH") {
                "    INDEX 00 00:00:00\r\n    INDEX 01 00:00:20\r\n"
            } else {
                "    INDEX 01 00:00:00\r\n"
            };
            logical.extend_from_slice(track);
            files.push((file, track.clone()));
        }
        let path = dir.join(format!("{name}.cue"));
        std::fs::write(&path, &cue).unwrap();
        files.push((format!("{name}.cue"), cue.into_bytes()));
        if sbi {
            let mut bytes = b"SBI\0".to_vec();
            bytes.extend_from_slice(&[0x00, 0x02, 0x10, 0x01]);
            bytes.extend_from_slice(&[0x41, 0x01, 0x01, 0x07, 0x06, 0x05, 0x00, 0x23, 0x08, 0x05]);
            std::fs::write(path.with_extension("sbi"), &bytes).unwrap();
            files.push((format!("{name}.sbi"), bytes));
        }
        (path, logical, files)
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

    /// SPU ADPCM of a tone, `blocks` 16-byte blocks of it.
    fn spu(blocks: usize) -> Vec<u8> {
        let (mut old, mut older) = (0i32, 0i32);
        let mut out = Vec::new();
        for b in 0..blocks {
            let mut block = [0u8; 16];
            block[0] = (2 << 4) | 6;
            for j in 0..28 {
                let n = b * 28 + j;
                let x = ((n as f64 * 0.02).sin() * 6000.0) as i32 + dither(n) * 64;
                let pred = (old * 115 - older * 52 + 32) >> 6;
                let t = ((x - pred) >> 6).clamp(-8, 7);
                block[2 + j / 2] |= ((t & 15) as u8) << ((j & 1) * 4);
                older = old;
                old = (((t << 12) >> 6) + pred).clamp(-32768, 32767);
            }
            out.extend_from_slice(&block);
        }
        out
    }

    /// A PlayStation 2 DVD of 600 sectors: its file system and
    /// `SYSTEM.CNF`, text, zeros, a `SShd` stream of 16-bit PCM, its
    /// channels 512 bytes a turn, and a `VAGp` stream.
    fn ps2_disc() -> Vec<u8> {
        const SECTOR: usize = 2048;
        let cnf = b"BOOT2 = cdrom0:\\SLUS_200.00;1\r\n";
        let mut iso = vec![0; 600 * SECTOR];
        let pvd = &mut iso[16 * SECTOR..17 * SECTOR];
        pvd[0] = 1;
        pvd[1..6].copy_from_slice(b"CD001");
        pvd[156..190].copy_from_slice(&record(&[0], 18, SECTOR as u32, true));
        let mut root = record(&[0], 18, SECTOR as u32, true);
        root.extend(record(&[1], 18, SECTOR as u32, true));
        root.extend(record(b"SYSTEM.CNF;1", 19, cnf.len() as u32, false));
        iso[18 * SECTOR..18 * SECTOR + root.len()].copy_from_slice(&root);
        iso[19 * SECTOR..19 * SECTOR + cnf.len()].copy_from_slice(cnf);
        iso[20 * SECTOR..120 * SECTOR].copy_from_slice(&text(100 * SECTOR, 40));
        let mut pcm = Vec::new();
        for period in 0..64 {
            for channel in 0..2 {
                for i in 0..256 {
                    let n = period * 256 + i;
                    let x = ((n as f64 * 0.01 + channel as f64).sin() * 9000.0) as i16
                        + dither(n) as i16 * 8;
                    pcm.extend_from_slice(&x.to_le_bytes());
                }
            }
        }
        let at = 200 * SECTOR;
        iso[at..at + 4].copy_from_slice(b"SShd");
        iso[at + 4..at + 8].copy_from_slice(&0x18u32.to_le_bytes());
        for (i, v) in [1u32, 48_000, 2, 512].iter().enumerate() {
            iso[at + 8 + 4 * i..at + 12 + 4 * i].copy_from_slice(&v.to_le_bytes());
        }
        iso[at + 0x20..at + 0x24].copy_from_slice(b"SSbd");
        iso[at + 0x24..at + 0x28].copy_from_slice(&(pcm.len() as u32).to_le_bytes());
        iso[at + 0x28..at + 0x28 + pcm.len()].copy_from_slice(&pcm);
        let at = 400 * SECTOR;
        // Above the 64 KiB an audio area takes at least
        let blocks = spu(5000);
        iso[at..at + 4].copy_from_slice(b"VAGp");
        iso[at + 0x0c..at + 0x10].copy_from_slice(&(blocks.len() as u32).to_be_bytes());
        iso[at + 0x10..at + 0x14].copy_from_slice(&22_050u32.to_be_bytes());
        iso[at + 0x30..at + 0x30 + blocks.len()].copy_from_slice(&blocks);
        iso
    }

    fn u64_at(bytes: &[u8], at: usize) -> u64 {
        u64::from_be_bytes(bytes[at..at + 8].try_into().unwrap())
    }

    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    /// `ore` with its index and metadata edited by `index` and `meta`, laid
    /// out again after its groups, the header's offsets, lengths and CRCs
    /// made to match.
    fn relaid(
        ore: &[u8],
        index: impl FnOnce(&mut Vec<u8>),
        meta: impl FnOnce(&mut Vec<u8>),
    ) -> Vec<u8> {
        let index_at = u64_at(ore, 48) as usize;
        let index_len = u64_at(ore, 56) as usize;
        let meta_at = u64_at(ore, 72) as usize;
        let meta_len = u32_at(ore, 80) as usize;
        let mut new_index = ore[index_at..index_at + index_len].to_vec();
        let mut new_meta = ore[meta_at..meta_at + meta_len].to_vec();
        index(&mut new_index);
        meta(&mut new_meta);
        let mut out = ore[..index_at].to_vec();
        out[56..64].copy_from_slice(&(new_index.len() as u64).to_be_bytes());
        out[64..68].copy_from_slice(&crc32c::crc32c(&new_index).to_be_bytes());
        out[72..80].copy_from_slice(&((index_at + new_index.len()) as u64).to_be_bytes());
        out[80..84].copy_from_slice(&(new_meta.len() as u32).to_be_bytes());
        out[84..88].copy_from_slice(&crc32c::crc32c(&new_meta).to_be_bytes());
        out.extend(new_index);
        out.extend(new_meta);
        out
    }

    /// The first record of an index as its kind and the bytes of its
    /// length, and the rest after them.
    fn first_record(index: &[u8]) -> (u8, Vec<u8>, Vec<u8>) {
        let end = 1
            + index[1..]
                .iter()
                .position(|&byte| byte & 0x80 == 0)
                .unwrap()
            + 1;
        (index[0], index[1..end].to_vec(), index[end..].to_vec())
    }

    #[test]
    #[ignore = "writes the vectors in spec/vectors, which are committed"]
    fn write_vectors() {
        let dir = vectors();
        // Everything again, but their license
        let license = std::fs::read(dir.join("LICENSE")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("LICENSE"), license).unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let tmp = tmp.path();
        let mut v = Vectors {
            dir: dir.clone(),
            lines: vec![
                "# SPEC.md §16: vector <path> ok|refuse <logical size> <logical BLAKE3> <what>"
                    .into(),
                "#              file <path> <output file> <size> <SHA-1>".into(),
            ],
        };

        // Cartridges, a codec and a profile each
        let rom = cartridge(1);
        let input = tmp.join("Vector.gb");
        std::fs::write(&input, &rom).unwrap();
        let gb = Options {
            platform: Some("gb"),
            ..Options::default()
        };
        v.compress(&input, "rom-zstd.ore", &gb, &[]);
        v.ok(
            "rom-zstd.ore",
            "class 17, zstd, CHUNK, DEDUP, ZERO, fill",
            &rom,
            &image("Vector.gb", &rom),
        );
        let input = tmp.join("Vector.nes");
        std::fs::write(&input, &rom).unwrap();
        let nes = Options {
            profile: Profile::Archive,
            codec: Codec::Lzma2,
            platform: Some("nes"),
        };
        v.compress(&input, "rom-lzma2.ore", &nes, &[]);
        v.ok(
            "rom-lzma2.ore",
            "class 17, lzma2, archive profile",
            &rom,
            &image("Vector.nes", &rom),
        );

        // A GameCube disc's padding, and a Wii partition's hashes
        let disc = gcwii::gamecube_disc(0x200000);
        let input = tmp.join("Vector (GameCube).iso");
        std::fs::write(&input, &disc).unwrap();
        v.compress(&input, "gamecube.ore", &Options::default(), &[]);
        v.ok(
            "gamecube.ore",
            "class 64, gc-wii-filler-prng",
            &disc,
            &image("Vector (GameCube).iso", &disc),
        );
        let disc = gcwii::wii_disc();
        let input = tmp.join("Vector (Wii).iso");
        std::fs::write(&input, &disc).unwrap();
        v.compress(&input, "wii.ore", &Options::default(), &[gcwii::COMMON_KEY]);
        v.ok(
            "wii.ore",
            "class 64, wii-partition, a hash exception",
            &disc,
            &image("Vector (Wii).iso", &disc),
        );

        // CDs: a Mode 2 track of every plane, a Mode 1 track, an audio
        // track, an SBI; a GD-ROM's two areas
        let cd_dir = tmp.join("cd");
        std::fs::create_dir(&cd_dir).unwrap();
        let mode2 = mode2_track(LEAD_IN);
        let after = LEAD_IN + (mode2.len() / SECTOR) as u32;
        let (cue, logical, files) = cue_disc(
            &cd_dir,
            "Vector",
            &[
                ("MODE2/2352", mode2, ""),
                ("MODE1/2352", mode1_track(16, after, 50), ""),
                ("AUDIO", audio_track(40), ""),
            ],
            true,
        );
        v.compress(&cue, "cd.ore", &Options::default(), &[]);
        v.ok(
            "cd.ore",
            "class 33, cd-data of every plane and sector kind, cd-audio, flac, xa, str of v2 and v3 frames, an SBI",
            &logical,
            &files,
        );
        let data_dir = tmp.join("cd-data");
        std::fs::create_dir(&data_dir).unwrap();
        let (cue, logical, files) = cue_disc(
            &data_dir,
            "Vector",
            &[("MODE1/2352", mode1_track(16, LEAD_IN, 80), "")],
            false,
        );
        v.compress(&cue, "cd-data.ore", &Options::default(), &[]);
        v.ok("cd-data.ore", "class 32, a Mode 1 track", &logical, &files);
        let gd_dir = tmp.join("gd");
        std::fs::create_dir(&gd_dir).unwrap();
        let (cue, logical, files) = cue_disc(
            &gd_dir,
            "Vector",
            &[
                (
                    "MODE1/2352",
                    mode1_track(30, LEAD_IN, 60),
                    "REM SINGLE-DENSITY AREA\r\n",
                ),
                ("AUDIO", audio_track(75), ""),
                (
                    "MODE1/2352",
                    mode1_track(40, LEAD_IN + 45_000, 70),
                    "REM HIGH-DENSITY AREA\r\n",
                ),
            ],
            false,
        );
        v.compress(&cue, "gd-rom.ore", &Options::default(), &[]);
        v.ok(
            "gd-rom.ore",
            "class 48, a GD-ROM's single- and high-density areas",
            &logical,
            &files,
        );

        // A PlayStation 2 DVD's audio, as pcm and vag
        let disc = ps2_disc();
        let input = tmp.join("Vector (PS2).iso");
        std::fs::write(&input, &disc).unwrap();
        v.compress(&input, "ps2.ore", &Options::default(), &[]);
        v.ok(
            "ps2.ore",
            "class 64, areas out of disc order, pcm, vag",
            &disc,
            &image("Vector (PS2).iso", &disc),
        );

        // A Wii U disc and an eShop title
        let disc = wiiu::wud();
        let input = tmp.join("Vector (Wii U).wud");
        std::fs::write(&input, &disc).unwrap();
        let keys = [wiiu::COMMON_KEY, wiiu::DISC_KEY, wiiu::VWII_KEY];
        v.compress(&input, "wiiu-disc.ore", &Options::default(), &keys);
        v.ok(
            "wiiu-disc.ore",
            "class 64, wiiu of every layer mode, repeat",
            &disc,
            &image("Vector (Wii U).wud", &disc),
        );
        let nus = tmp.join("Vector (Wii U Title)");
        std::fs::create_dir(&nus).unwrap();
        wiiu::nus(&nus);
        // The folder's files, in name order, as the title's logical stream
        let mut title: Vec<(String, Vec<u8>)> = std::fs::read_dir(&nus)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap().to_str().unwrap().to_owned();
                (name, std::fs::read(&path).unwrap())
            })
            .collect();
        title.sort();
        v.compress(
            &nus,
            "wiiu-title.ore",
            &Options::default(),
            &[wiiu::COMMON_KEY],
        );
        let logical: Vec<u8> = title
            .iter()
            .filter(|(name, _)| name != "title.tik")
            .flat_map(|(_, bytes)| bytes.clone())
            .collect();
        let files: Vec<(String, Vec<u8>)> = title
            .into_iter()
            .map(|(name, bytes)| (format!("Vector (Wii U Title)/{name}"), bytes))
            .collect();
        v.ok(
            "wiiu-title.ore",
            "class 97, a title's files, its ticket",
            &logical,
            &files,
        );

        // A store, its segments sealed, and two ROMs sharing their text
        let store = dir.join("store").join(ore_rs::STORE_DIR);
        ore_rs::create_store(&store, &StoreOptions::default()).unwrap();
        for (name, seed) in [("Vector A.gb", 1), ("Vector B.gb", 2)] {
            let mut rom = cartridge(1);
            rom[..4096].copy_from_slice(&text(4096, seed + 100));
            let input = tmp.join(name);
            std::fs::write(&input, &rom).unwrap();
            let path = format!("store/{}", name.replace(".gb", ".ore"));
            ore_rs::compress_to_store(&input, &dir.join(&path), &store, &gb, &[], &mut |_| {})
                .unwrap();
            v.ok(
                &path,
                "a thin ORE, STORE records, a sealed segment's parity",
                &rom,
                &image(name, &rom),
            );
        }

        // What a later writer adds, read past (§14)
        let ore = std::fs::read(dir.join("rom-zstd.ore")).unwrap();
        let files = image("Vector.gb", &rom);
        let mut later = |name: &str, why: &str, edit: &dyn Fn(&mut Vec<u8>)| {
            let mut bytes = ore.clone();
            edit(&mut bytes);
            let path = format!("compat/{name}");
            std::fs::create_dir_all(dir.join("compat")).unwrap();
            std::fs::write(dir.join(&path), bytes).unwrap();
            v.ok(&path, why, &rom, &files);
        };
        later("spec-version.ore", "a higher spec_version", &|b| {
            b[4..6].copy_from_slice(&2u16.to_be_bytes())
        });
        later("optional-flag.ore", "an unknown flags_optional bit", &|b| {
            b[12..16].copy_from_slice(&(1u32 << 31).to_be_bytes())
        });
        later("reserved.ore", "filled reserved fields", &|b| {
            b[21..24].fill(0x5A);
            b[68..72].fill(0x5A);
            b[120..184].fill(0x5A);
        });
        later("long-header.ore", "a header of 256 bytes", &|b| {
            b[6..8].copy_from_slice(&256u16.to_be_bytes());
            for at in [48, 72] {
                let offset = u64_at(b, at) + 64;
                b[at..at + 8].copy_from_slice(&offset.to_be_bytes());
            }
            b.splice(192..192, [0xC3; 64]);
        });
        later("appended.ore", "bytes after the metadata", &|b| {
            b.extend(std::iter::repeat_n(0xA5, 4096))
        });
        later("meta-key.ore", "an unknown metadata key", &|b| {
            *b = relaid(
                b,
                |_| {},
                |meta| {
                    // The top map one entry longer, ending with "later": 1
                    assert_eq!(meta[0], 0xA4);
                    meta[0] = 0xA5;
                    meta.extend_from_slice(&[0x65, b'l', b'a', b't', b'e', b'r', 0x01]);
                },
            )
        });
        later(
            "meta-order.ore",
            "metadata keys not in canonical order",
            &|b| {
                *b = relaid(
                    b,
                    |_| {},
                    |meta| {
                        // The media map's keys in reversed canonical
                        // order: a permutation no writer emits, which
                        // the reader must still take
                        let mut value: ciborium::Value = ciborium::from_reader(&meta[..]).unwrap();
                        let ciborium::Value::Map(top) = &mut value else {
                            unreachable!("the metadata is one map")
                        };
                        let media = top
                            .iter_mut()
                            .find(|(key, _)| key.as_text() == Some("media"))
                            .unwrap();
                        let ciborium::Value::Map(keys) = &mut media.1 else {
                            unreachable!("media is a map")
                        };
                        keys.reverse();
                        *meta = Vec::new();
                        ciborium::into_writer(&value, meta).unwrap();
                    },
                )
            },
        );

        // What a reader refuses (§13)
        let mut refused = |name: &str, why: &str, edit: &dyn Fn(&mut Vec<u8>)| {
            let mut bytes = ore.clone();
            edit(&mut bytes);
            let path = format!("hostile/{name}");
            std::fs::create_dir_all(dir.join("hostile")).unwrap();
            std::fs::write(dir.join(&path), bytes).unwrap();
            v.refuse(&path, why);
        };
        let index_at = u64_at(&ore, 48) as usize;
        let meta_at = u64_at(&ore, 72) as usize;
        refused("layout-2.ore", "the magic of a later layout, ORE2", &|b| {
            b[3] = b'2'
        });
        refused("magic.ore", "not an ORE's magic", &|b| {
            b[0..4].copy_from_slice(b"RIFF")
        });
        refused("required-flag.ore", "an unknown flags_required bit", &|b| {
            b[8..12].copy_from_slice(&(1u32 << 31).to_be_bytes())
        });
        refused("short-header.ore", "a header_len under 192", &|b| {
            b[6..8].copy_from_slice(&128u16.to_be_bytes())
        });
        refused("media-class.ore", "an unknown media class", &|b| {
            b[18..20].copy_from_slice(&255u16.to_be_bytes())
        });
        refused("objects.ore", "an object_count of 2", &|b| {
            b[44..48].copy_from_slice(&2u32.to_be_bytes())
        });
        refused(
            "size.ore",
            "a logical_size the metadata does not have",
            &|b| {
                let size = u64_at(b, 24) + 1;
                b[24..32].copy_from_slice(&size.to_be_bytes());
            },
        );
        refused(
            "logical-hash.ore",
            "a logical_hash the stream does not have",
            &|b| b[100] ^= 1,
        );
        refused("index-crc.ore", "an index failing its CRC", &|b| {
            b[index_at] ^= 1
        });
        refused("meta-crc.ore", "metadata failing its CRC", &|b| {
            b[meta_at + 1] ^= 1
        });
        refused("truncated.ore", "regions past the end of the file", &|b| {
            b.truncate(index_at + 4)
        });
        refused(
            "envelope.ore",
            "a group's envelope of the wrong magic",
            &|b| b[192] = b'X',
        );
        refused("group-crc.ore", "a group failing its CRC", &|b| {
            b[192 + 40] ^= 1
        });
        refused(
            "index-tail.ore",
            "an index holding more than it describes",
            &|b| *b = relaid(b, |index| index.push(0), |_| {}),
        );
        refused("record-kind.ore", "a record of a reserved kind, 3", &|b| {
            *b = relaid(b, |index| index[0] = 3, |_| {})
        });
        refused("record-len.ore", "a record of length 0", &|b| {
            *b = relaid(
                b,
                |index| {
                    let (kind, _, rest) = first_record(index);
                    *index = [vec![kind, 0], rest].concat();
                },
                |_| {},
            )
        });
        refused(
            "dedup-forward.ore",
            "a DEDUP of a chunk not stored before it",
            &|b| {
                *b = relaid(
                    b,
                    |index| {
                        let (_, len, rest) = first_record(index);
                        *index = [vec![5], len, vec![0], rest].concat();
                    },
                    |_| {},
                )
            },
        );
        refused(
            "store-record.ore",
            "a STORE record in a standalone ORE",
            &|b| {
                *b = relaid(
                    b,
                    |index| {
                        let (_, len, rest) = first_record(index);
                        *index = [vec![9], len, vec![0], rest].concat();
                    },
                    |_| {},
                )
            },
        );
        refused("area-kind.ore", "an unknown area kind", &|b| {
            *b = relaid(
                b,
                |_| {},
                |meta| {
                    let at = meta.windows(4).position(|w| w == b"\x63raw").unwrap();
                    meta[at + 3] = b'x';
                },
            )
        });

        let mut manifest = v.lines.join("\n");
        manifest.push('\n');
        std::fs::write(dir.join("manifest.tsv"), manifest).unwrap();
    }
}

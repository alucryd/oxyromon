//! A CD comes back out of an ORE as the files it went in as, byte for byte:
//! its cue sheet, a BIN a track, its SBI; alone or through a store.

use std::path::{Path, PathBuf};

use ore_rs::{Codec, Options, Profile, Reader, StoreOptions};
use sha1::{Digest, Sha1};
use tempfile::TempDir;

// The cd crate the library embeds, to build sectors as a disc holds them
#[allow(dead_code)]
#[path = "../src/cd/lib.rs"]
mod cd;
use cd::sector::{self, Kind, LEAD_IN, SECTOR};

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

/// A video sector of a still picture, 16x16, 4 sectors a frame: the
/// first holds the frame, BS v2, six blocks of a DC of 0 and no AC.
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

/// A data track of every kind of sector a PlayStation disc holds, a file
/// on it twice, and a still picture as STR video, a group of it.
fn data_track(seed: u64) -> Vec<u8> {
    let file = noise(100 * 2048, 7);
    let mut track = Vec::new();
    for i in 0..1113u32 {
        let frame = LEAD_IN + i;
        let (kind, subheader, data) = match i {
            100..200 => (Kind::Form1, [0, 0, 8, 0, 0, 0, 8, 0], {
                let at = (i - 100) as usize * 2048;
                file[at..at + 2048].to_vec()
            }),
            300..400 => (Kind::Form1, [0, 0, 8, 0, 0, 0, 8, 0], {
                let at = (i - 300) as usize * 2048;
                file[at..at + 2048].to_vec()
            }),
            200..250 => (
                Kind::Form2,
                [1, 1, 0x64, 1, 1, 1, 0x64, 1],
                noise(2324, seed + i as u64),
            ),
            250..270 => (
                Kind::Form2NoEdc,
                [1, 2, 0x64, 1, 1, 2, 0x64, 1],
                noise(2324, i as u64),
            ),
            600..1112 => (
                Kind::Form1,
                [1, 1, 0x48, 0, 1, 1, 0x48, 0],
                still((i - 600) / 4, ((i - 600) % 4) as u16),
            ),
            // Copies that differ, as Alundra 2 has them
            270 => (
                Kind::Form2NoEdc,
                [1, 3, 0x64, 1, 1, 1, 0x64, 1],
                noise(2324, 1),
            ),
            _ => (
                Kind::Form1,
                [0, 0, 8, 0, 0, 0, 8, 0],
                noise(2048, seed + i as u64),
            ),
        };
        let mut raw = [0; SECTOR];
        sector::rebuild(kind, frame, &subheader, &data, &mut raw);
        if i == 1112 {
            // A mastering terminator: a Mode 2 sector nothing regenerates
            raw[16..].fill(0);
            raw[0x900] = 0x5A;
        }
        track.extend_from_slice(&raw);
    }
    track
}

/// An audio track: a tone with some noise, then silence.
fn audio_track(sectors: usize, seed: u64) -> Vec<u8> {
    let mut pcm = Vec::with_capacity(sectors * SECTOR);
    let noise = noise(sectors * 588, seed);
    for (i, &noise) in noise.iter().enumerate() {
        let sample: i16 = if i < sectors * 588 * 3 / 4 {
            ((i as i32 * 211 % 4000) - 2000 + noise as i32) as i16
        } else {
            0
        };
        pcm.extend_from_slice(&sample.to_le_bytes());
        pcm.extend_from_slice(&(sample / 2).to_le_bytes());
    }
    pcm
}

/// A disc as Redump lists it, in `dir`: its cue sheet's path.
fn disc(dir: &Path, name: &str, seed: u64, sbi: bool) -> PathBuf {
    let tracks = [data_track(seed), audio_track(300, 5), audio_track(300, 5)];
    let mut cue = String::new();
    for (i, track) in tracks.iter().enumerate() {
        let file = format!("{name} (Track {}).bin", i + 1);
        std::fs::write(dir.join(&file), track).unwrap();
        cue += &format!("FILE \"{file}\" BINARY\r\n");
        if i == 0 {
            cue += "  TRACK 01 MODE2/2352\r\n    INDEX 01 00:00:00\r\n";
        } else {
            cue += &format!(
                "  TRACK {:02} AUDIO\r\n    INDEX 00 00:00:00\r\n    INDEX 01 00:02:00\r\n",
                i + 1
            );
        }
    }
    let path = dir.join(format!("{name}.cue"));
    std::fs::write(&path, cue).unwrap();
    if sbi {
        let mut bytes = b"SBI\0".to_vec();
        bytes.extend_from_slice(&[0x03, 0x08, 0x05, 0x01]);
        bytes.extend_from_slice(&[0x41, 0x01, 0x01, 0x07, 0x06, 0x05, 0x00, 0x23, 0x08, 0x05]);
        std::fs::write(path.with_extension("sbi"), bytes).unwrap();
    }
    path
}

/// The files of a disc beside its cue sheet: their names and bytes.
fn files(cue: &Path) -> Vec<(String, Vec<u8>)> {
    let stem = cue.file_stem().unwrap().to_string_lossy().into_owned();
    let mut files: Vec<_> = std::fs::read_dir(cue.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(&stem)
        })
        .map(|path| {
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read(&path).unwrap(),
            )
        })
        .collect();
    files.sort();
    files
}

/// Decompress `ore` into a directory of its own, check it gives back the
/// files beside `cue`, and remove them.
fn check(ore: &Path, cue: &Path) {
    let out = TempDir::new().unwrap();
    let back = out.path().join(cue.file_name().unwrap());
    ore_rs::decompress(ore, &back, &mut |_| {}).unwrap();
    let (want, got) = (files(cue), files(&back));
    assert_eq!(
        want.iter().map(|f| &f.0).collect::<Vec<_>>(),
        got.iter().map(|f| &f.0).collect::<Vec<_>>()
    );
    for ((name, want), (_, got)) in want.iter().zip(&got) {
        assert!(want == got, "{name} differs");
    }
}

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

#[test]
fn a_cd_round_trips_as_its_files() {
    let dir = TempDir::new().unwrap();
    let cue = disc(dir.path(), "Game (Europe)", 1, true);
    let size: u64 = files(&cue)
        .iter()
        .filter(|(name, _)| name.ends_with(".bin"))
        .map(|(_, bytes)| bytes.len() as u64)
        .sum();
    let ore = dir.path().join("game.ore");
    for options in &OPTIONS {
        let mut consumed = 0;
        let stats = ore_rs::compress(&cue, &ore, options, &[], &mut |n| consumed += n).unwrap();
        assert_eq!(consumed, size, "progress adds up to the tracks");
        // The repeated file, the second audio track and the silence cost
        // nothing; the generated parts of sectors neither
        assert!(
            stats.output_size < size / 2,
            "{options:?}: {}",
            stats.output_size
        );
        check(&ore, &cue);
    }
}

#[test]
fn a_cd_records_each_files_hashes_and_reads_anywhere() {
    let dir = TempDir::new().unwrap();
    let cue = disc(dir.path(), "Game (Europe)", 1, true);
    let ore = dir.path().join("game.ore");
    ore_rs::compress(&cue, &ore, &Options::default(), &[], &mut |_| {}).unwrap();
    let mut reader = Reader::open(&ore).unwrap();
    assert!(reader.is_cd());
    for (name, bytes) in files(&cue) {
        let sha1 = Sha1::digest(&bytes).to_vec();
        assert!(
            reader
                .identity()
                .any(|(view, algo, value)| view == name && algo == "sha-1" && value == sha1),
            "{name}"
        );
    }
    // The tracks one after the other, as the logical stream
    let stream: Vec<u8> = files(&cue)
        .into_iter()
        .filter(|(name, _)| name.ends_with(".bin"))
        .flat_map(|(_, bytes)| bytes)
        .collect();
    assert_eq!(reader.size(), stream.len() as u64);
    for (at, len) in [
        (0, 10),
        (SECTOR * 99 + 7, SECTOR * 3),
        // Form 1 into XA, from one plane into the other
        (SECTOR * 198 + 5, SECTOR * 5),
        // Data into video
        (SECTOR * 598, SECTOR * 5),
        (SECTOR * 1110, SECTOR * 5),
        (stream.len() - 9, 9),
    ] {
        let mut buf = vec![0; len];
        reader.read_at(at as u64, &mut buf).unwrap();
        assert!(buf == stream[at..at + len], "read at {at}");
    }
}

#[test]
fn cds_share_a_store_and_leave_it() {
    let dir = TempDir::new().unwrap();
    let store = dir.path().join(ore_rs::STORE_DIR);
    ore_rs::create_store(&store, &StoreOptions::default()).unwrap();
    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    // Two versions of a game: its audio and most of its data the same
    let (cue_a, cue_b) = (
        disc(&a, "Game (Europe)", 1, true),
        disc(&b, "Game (USA)", 2, false),
    );
    let (ore_a, ore_b) = (dir.path().join("a.ore"), dir.path().join("b.ore"));
    let options = Options::default();
    ore_rs::compress_to_store(&cue_a, &ore_a, &store, &options, &[], &mut |_| {}).unwrap();
    let segments = |store: &Path| -> u64 {
        std::fs::read_dir(store)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|x| x == "ores"))
            .map(|path| std::fs::metadata(path).unwrap().len())
            .sum()
    };
    let first = segments(&store);
    ore_rs::compress_to_store(&cue_b, &ore_b, &store, &options, &[], &mut |_| {}).unwrap();
    // What b adds is its own sectors' data, little against a's
    assert!(segments(&store) - first < first / 2);
    check(&ore_a, &cue_a);
    check(&ore_b, &cue_b);

    let detached = dir.path().join("b-alone.ore");
    ore_rs::detach(&ore_b, &detached, &options, &mut |_| {}).unwrap();
    check(&detached, &cue_b);
    let again = dir.path().join("b-again.ore");
    ore_rs::attach(&detached, &again, &store, &options, &mut |_| {}).unwrap();
    check(&again, &cue_b);

    // Compacting the store keeps audio as audio
    std::fs::remove_file(&ore_b).unwrap();
    std::fs::remove_file(&again).unwrap();
    ore_rs::purge(&store, &options, 0.0).unwrap();
    check(&ore_a, &cue_a);
    assert_eq!(ore_rs::check_store(&store, false).unwrap().lost_groups, 0);
}

#[test]
fn a_cue_sheet_naming_one_file_for_two_tracks_is_refused() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("g.bin"), vec![0; SECTOR * 4]).unwrap();
    let cue = dir.path().join("g.cue");
    std::fs::write(
        &cue,
        "FILE \"g.bin\" BINARY\n TRACK 01 MODE2/2352\n  INDEX 01 00:00:00\n TRACK 02 AUDIO\n  INDEX 01 00:00:02\n",
    )
    .unwrap();
    let error = ore_rs::compress(
        &cue,
        &dir.path().join("g.ore"),
        &Options::default(),
        &[],
        &mut |_| {},
    )
    .unwrap_err();
    assert!(matches!(error, ore_rs::Error::Unsupported(_)), "{error}");
}

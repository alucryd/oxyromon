//! CD creation agreement with the reference `chdman createcd` (0.289): for
//! CUE, GDI and ISO inputs covering the parser's paths, the CHDs `chdrs`
//! writes are byte-identical to `chdman`'s. Where the bytes depend on the
//! compression library rather than on this port (see the codec lists
//! below) the header's hashes, which cover the data and the metadata, are
//! compared instead.
//! Skipped unless a `chdman` binary is reachable through `$CHDMAN` or
//! `$PATH`.

#![cfg(feature = "cli")]

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

fn chdman_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CHDMAN").map(PathBuf::from) {
        return path.is_file().then_some(path);
    }
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join("chdman"))
        .find(|path| path.is_file())
}

fn run(bin: &Path, dir: &Path, args: &[&str]) {
    let out = Command::new(bin)
        .current_dir(dir)
        .args(args)
        .output()
        .expect("failed to run");
    assert!(
        out.status.success(),
        "{} {:?} failed: {}{}",
        bin.display(),
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `frames` frames of `size` bytes: a repeating pattern, so codecs have
/// something to find, with stretches of noise so not every hunk compresses.
fn frames(frames: usize, size: usize, seed: u64) -> Vec<u8> {
    let mut data = vec![0u8; frames * size];
    let mut state = seed | 1;
    for (i, byte) in data.iter_mut().enumerate() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = if (i / 4096) % 3 == 2 {
            (state >> 24) as u8
        } else {
            (i % 251) as u8 ^ (seed as u8)
        };
    }
    data
}

/// A 16-bit stereo 44.1 kHz WAV around `samples`, with a chunk before the
/// format one that the parser has to skip.
fn wav(samples: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(b"WAVE");
    body.extend_from_slice(b"LIST");
    body.extend_from_slice(&4u32.to_le_bytes());
    body.extend_from_slice(b"INFO");
    body.extend_from_slice(b"fmt ");
    body.extend_from_slice(&16u32.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&2u16.to_le_bytes());
    body.extend_from_slice(&44100u32.to_le_bytes());
    body.extend_from_slice(&(44100u32 * 4).to_le_bytes());
    body.extend_from_slice(&4u16.to_le_bytes());
    body.extend_from_slice(&16u16.to_le_bytes());
    body.extend_from_slice(b"data");
    body.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    body.extend_from_slice(samples);
    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend_from_slice(&body);
    file
}

/// Writes each `(name, bytes)` into `dir`.
fn files(dir: &Path, files: &[(&str, Vec<u8>)]) {
    for (name, data) in files {
        std::fs::write(dir.join(name), data).unwrap();
    }
}

/// A fixture: a name, the table of contents to compress, and its files.
type Fixture = (&'static str, &'static str, Vec<(&'static str, Vec<u8>)>);

fn fixtures() -> Vec<Fixture> {
    vec![
        (
            "single",
            "disc.cue",
            vec![
                (
                    "disc.cue",
                    b"FILE \"disc.bin\" BINARY\n  TRACK 01 MODE1/2048\n    INDEX 01 00:00:00\n"
                        .to_vec(),
                ),
                ("disc.bin", frames(130, 2048, 1)),
            ],
        ),
        (
            // one BIN: lengths from the next track's index 0, a pregap in
            // the file, a pregap and postgap outside it, and flags
            "shared bin",
            "disc.cue",
            vec![
                (
                    "disc.cue",
                    b"FILE \"disc image.bin\" BINARY\r\n\
                      TRACK 01 MODE1/2352\r\n  INDEX 01 00:00:00\r\n\
                      TRACK 02 AUDIO\r\n  FLAGS DCP PRE\r\n  INDEX 00 00:02:00\r\n  INDEX 01 00:03:00\r\n\
                      TRACK 03 AUDIO\r\n  PREGAP 00:02:00\r\n  INDEX 01 00:06:00\r\n  POSTGAP 00:01:00\r\n"
                        .to_vec(),
                ),
                ("disc image.bin", frames(601, 2352, 2)),
            ],
        ),
        (
            // a BIN per track, the second holding its own pregap
            "split bins",
            "disc.cue",
            vec![
                (
                    "disc.cue",
                    b"FILE \"t1.bin\" BINARY\n TRACK 01 MODE2/2352\n  INDEX 01 00:00:00\n\
                      FILE \"t2.bin\" BINARY\n TRACK 02 AUDIO\n  INDEX 00 00:00:00\n  INDEX 01 00:02:00\n\
                      FILE \"t3.bin\" BINARY\n TRACK 03 AUDIO\n  INDEX 01 00:00:00\n"
                        .to_vec(),
                ),
                ("t1.bin", frames(203, 2352, 3)),
                ("t2.bin", frames(301, 2352, 4)),
                ("t3.bin", frames(77, 2352, 5)),
            ],
        ),
        (
            // subcode carried in the file (CD+G tracks postdate chdman 0.289)
            "subcode",
            "disc.cue",
            vec![
                (
                    "disc.cue",
                    b"FILE \"t1.bin\" BINARY\n TRACK 01 MODE1/2352 RW_RAW\n  INDEX 01 00:00:00\n\
                      FILE \"t2.bin\" BINARY\n TRACK 02 AUDIO RW\n  INDEX 01 00:00:00\n"
                        .to_vec(),
                ),
                ("t1.bin", frames(90, 2448, 6)),
                ("t2.bin", frames(45, 2448, 7)),
            ],
        ),
        (
            "wave",
            "disc.cue",
            vec![
                (
                    "disc.cue",
                    b"FILE \"t1.bin\" BINARY\n TRACK 01 MODE1/2048\n  INDEX 01 00:00:00\n\
                      FILE \"t2.wav\" WAVE\n TRACK 02 AUDIO\n  INDEX 01 00:00:00\n"
                        .to_vec(),
                ),
                ("t1.bin", frames(50, 2048, 8)),
                ("t2.wav", wav(&frames(120, 2352, 9))),
            ],
        ),
        (
            // more frames than chdman's work buffer holds, ending half way
            // through a hunk
            "long",
            "disc.cue",
            vec![
                (
                    "disc.cue",
                    b"FILE \"disc.bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n"
                        .to_vec(),
                ),
                ("disc.bin", frames(2052, 2352, 20)),
            ],
        ),
        (
            "iso",
            "disc.iso",
            vec![("disc.iso", frames(333, 2048, 10))],
        ),
        (
            // a GD-ROM: a virtual pregap, then padding up to the
            // high-density area
            "gdi",
            "disc.gdi",
            vec![
                (
                    "disc.gdi",
                    b"3\n1 0 4 2352 track01.bin 0\n2 450 0 2352 \"track 02.raw\" 0\n3 45000 4 2352 track03.bin 0\n"
                        .to_vec(),
                ),
                ("track01.bin", frames(300, 2352, 11)),
                ("track 02.raw", frames(200, 2352, 12)),
                ("track03.bin", frames(500, 2352, 13)),
            ],
        ),
        (
            // the Redump layout of a GD-ROM: one CUE, both density areas
            "gd cue",
            "disc.cue",
            vec![
                (
                    "disc.cue",
                    b"REM SINGLE-DENSITY AREA\nFILE \"t1.bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n\
                      FILE \"t2.bin\" BINARY\n  TRACK 02 AUDIO\n    INDEX 00 00:00:00\n    INDEX 01 00:02:00\n\
                      REM HIGH-DENSITY AREA\nFILE \"t3.bin\" BINARY\n  TRACK 03 MODE1/2352\n    INDEX 01 00:00:00\n"
                        .to_vec(),
                ),
                ("t1.bin", frames(300, 2352, 14)),
                ("t2.bin", frames(375, 2352, 15)),
                ("t3.bin", frames(220, 2352, 16)),
            ],
        ),
        (
            // a single-BIN multisession disc: the lead-out trims the first
            // session, and the gap up to the second pads it
            "multisession bin",
            "disc.cue",
            vec![
                (
                    "disc.cue",
                    b"REM SESSION 01\nFILE \"disc.bin\" BINARY\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n\
                      REM LEAD-OUT 00:04:00\nREM SESSION 02\n  TRACK 02 MODE2/2352\n    INDEX 00 00:06:00\n    INDEX 01 00:08:00\n"
                        .to_vec(),
                ),
                ("disc.bin", frames(900, 2352, 17)),
            ],
        ),
        (
            // a BIN per session: standard lead-out and lead-in
            "multisession bins",
            "disc.cue",
            vec![
                (
                    "disc.cue",
                    b"REM SESSION 01\nFILE \"t1.bin\" BINARY\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n\
                      REM SESSION 02\nREM PREGAP 00:02:00\nFILE \"t2.bin\" BINARY\n  TRACK 02 MODE1/2352\n    INDEX 01 00:00:00\n"
                        .to_vec(),
                ),
                ("t1.bin", frames(400, 2352, 18)),
                ("t2.bin", frames(250, 2352, 19)),
            ],
        ),
    ]
}

/// Codecs compared byte for byte; those built on deflate, compared byte for
/// byte only with `CHDRS_STRICT_PARITY` set (where chdman links zlib-ng,
/// whose output zlib-rs reproduces); and those built on LZMA, whose encoder
/// is lzma-rust2's rather than MAME's LZMA SDK one. The last two are checked
/// by the header's hashes, which cover the data and the metadata, and by
/// chdman verifying them.
const IDENTICAL: &[&str] = &["none", "cdzs", "zstd"];
const DEFLATE: &[&str] = &["cdzl", "cdfl", "zlib"];
const LZMA: &[&str] = &["cdlz,cdzl,cdfl", "cdlz", "lzma"];

/// `chd_rs::create_cd` with chdman's default hunk and the `-c` codecs.
fn create_cd(toc: &Path, output: &Path, codecs: &str) {
    let mut slots = [0u32; 4];
    for (slot, name) in slots.iter_mut().zip(codecs.split(',')) {
        if name != "none" {
            *slot = u32::from_be_bytes(name.as_bytes().try_into().unwrap());
        }
    }
    chd_rs::create_cd(
        toc,
        output,
        8 * chd_rs::CD_FRAME_SIZE,
        slots,
        None,
        &mut |_| {},
    )
    .unwrap();
}

#[test]
fn cd_chds_match_chdman() {
    let Some(chdman) = chdman_binary() else {
        eprintln!("skipping: chdman not found");
        return;
    };
    let chdrs = PathBuf::from(env!("CARGO_BIN_EXE_chdrs"));
    for (name, toc, inputs) in fixtures() {
        let dir = TempDir::new().unwrap();
        files(dir.path(), &inputs);
        let ours = dir.path().join(Path::new(toc).with_extension("chd"));
        let theirs = dir.path().join("theirs.chd");
        let strict = std::env::var_os("CHDRS_STRICT_PARITY").is_some();
        for codecs in IDENTICAL.iter().chain(DEFLATE).chain(LZMA) {
            let _ = std::fs::remove_file(&ours);
            let _ = std::fs::remove_file(&theirs);
            run(
                &chdman,
                dir.path(),
                &["createcd", "-i", toc, "-o", "theirs.chd", "-c", codecs],
            );
            if toc.ends_with(".iso") {
                // the CLI takes an ISO for a DVD or a hard disk
                create_cd(&dir.path().join(toc), &ours, codecs);
            } else {
                run(&chdrs, dir.path(), &["-c", codecs, toc]);
            }
            if !IDENTICAL.contains(codecs) {
                run(
                    &chdman,
                    dir.path(),
                    &["verify", "-i", &ours.to_string_lossy()],
                );
            }
            let ours = std::fs::read(&ours).unwrap();
            let theirs = std::fs::read(&theirs).unwrap();
            if IDENTICAL.contains(codecs) || (strict && DEFLATE.contains(codecs)) {
                assert!(ours == theirs, "{name} with {codecs} differs from chdman");
            } else {
                // the raw and overall SHA-1s of the header
                assert_eq!(ours[64..104], theirs[64..104], "{name} with {codecs}");
            }
        }
    }
}

#[test]
fn a_cue_naming_a_missing_bin_fails_cleanly() {
    let dir = TempDir::new().unwrap();
    files(
        dir.path(),
        &[(
            "disc.cue",
            b"FILE \"gone.bin\" BINARY\n TRACK 01 MODE1/2048\n  INDEX 01 00:00:00\n".to_vec(),
        )],
    );
    let output = dir.path().join("disc.chd");
    let error = chd_rs::create_cd(
        &dir.path().join("disc.cue"),
        &output,
        8 * chd_rs::CD_FRAME_SIZE,
        [u32::from_be_bytes(*b"cdzl"), 0, 0, 0],
        None,
        &mut |_| {},
    )
    .unwrap_err();
    assert!(error.to_string().contains("gone.bin"), "{error}");
    assert!(!output.exists());
    assert!(!dir.path().join("disc.chd.part").exists());
}

#[test]
fn progress_adds_up_to_the_input_size() {
    let dir = TempDir::new().unwrap();
    let (_, toc, inputs) = fixtures().swap_remove(2);
    files(dir.path(), &inputs);
    let toc = dir.path().join(toc);
    let mut seen = 0;
    chd_rs::create_cd(
        &toc,
        &dir.path().join("disc.chd"),
        8 * chd_rs::CD_FRAME_SIZE,
        [u32::from_be_bytes(*b"cdzl"), 0, 0, 0],
        None,
        &mut |bytes| seen += bytes,
    )
    .unwrap();
    assert_eq!(seen, chd_rs::cd_input_size(&toc).unwrap());
    assert_eq!(seen, (203 + 301 + 77) * 2352);
}

/// The files of a directory, by name.
fn listing(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                std::fs::read(entry.path()).unwrap(),
            )
        })
        .collect();
    files.sort();
    files
}

#[test]
fn cd_extraction_matches_chdman() {
    let Some(chdman) = chdman_binary() else {
        eprintln!("skipping: chdman not found");
        return;
    };
    for (name, toc, inputs) in fixtures() {
        let dir = TempDir::new().unwrap();
        files(dir.path(), &inputs);
        run(
            &chdman,
            dir.path(),
            &["createcd", "-i", toc, "-o", "disc.chd", "-c", "cdzs"],
        );
        for sheet in ["disc.cue", "disc.gdi", "disc.toc"] {
            let theirs = dir.path().join(format!("theirs-{sheet}"));
            let ours = dir.path().join(format!("ours-{sheet}"));
            std::fs::create_dir(&theirs).unwrap();
            std::fs::create_dir(&ours).unwrap();
            run(
                &chdman,
                dir.path(),
                &[
                    "extractcd",
                    "-i",
                    "disc.chd",
                    "-o",
                    &format!("theirs-{sheet}/{sheet}"),
                ],
            );
            let mut chd = chd_rs::Chd::open(dir.path().join("disc.chd")).unwrap();
            let mut seen = 0;
            chd_rs::extract_cd(&mut chd, &ours.join(sheet), None, false, &mut |bytes| {
                seen += bytes
            })
            .unwrap();
            assert_eq!(seen, chd.info().logical_size, "{name} to {sheet}");
            let (theirs, ours) = (listing(&theirs), listing(&ours));
            assert_eq!(
                theirs.iter().map(|file| &file.0).collect::<Vec<_>>(),
                ours.iter().map(|file| &file.0).collect::<Vec<_>>(),
                "{name} to {sheet}: different files"
            );
            for ((file, theirs), (_, ours)) in theirs.iter().zip(&ours) {
                if file.ends_with(sheet.rsplit('.').next().unwrap()) {
                    assert_eq!(
                        String::from_utf8_lossy(ours),
                        String::from_utf8_lossy(theirs),
                        "{name}: {file} differs"
                    );
                }
                assert!(ours == theirs, "{name}: {file} differs");
            }
        }
    }
}

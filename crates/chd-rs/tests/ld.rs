//! LaserDisc agreement with the reference `chdman` (0.289): for AVIs of
//! every video and audio layout chdman reads, the CHDs `chdrs` creates are
//! byte-identical to `chdman createld`'s, and the AVIs it extracts to
//! `chdman extractld`'s. The AVIs are made with `ffmpeg`; the tests skip
//! rather than fail without it or without a `chdman` binary (`$CHDMAN`,
//! then `$PATH`).

#![cfg(feature = "cli")]

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

fn binary(name: &str, variable: &str) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(variable).map(PathBuf::from) {
        return path.is_file().then_some(path);
    }
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(name))
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

/// A line of 24-bit Manchester code, a 1 rising mid-cell, the way
/// LaserDiscs carry their Philips codes on lines 16 to 18.
fn code_line(width: usize, code: u32, clock: f64, start: f64, seed: &mut u64) -> Vec<u8> {
    let mut line = vec![16u8; width];
    for bit in 0..24 {
        let one = (code >> (23 - bit)) & 1 == 1;
        let cell = start + f64::from(bit) * clock;
        let mut x = cell as usize;
        while (x as f64) < cell + clock && x < width {
            let first_half = (x as f64 - cell) < clock / 2.0;
            line[x] = if first_half != one { 235 } else { 16 };
            x += 1;
        }
    }
    for value in &mut line {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *value = value.saturating_add((*seed % 7) as u8).saturating_sub(3);
    }
    line
}

/// Raw YUYV frames of an NTSC LaserDisc capture with VBI codes: white
/// flags on line 11 of some fields, a lead-in or chapter code on line 16,
/// and on lines 17 and 18 picture numbers that sometimes disagree, so every
/// way of choosing between them is taken.
fn vbi_frames(frames: usize) -> Vec<u8> {
    let (width, height) = (720, 524);
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut out = Vec::new();
    for frame in 0..frames {
        let mut rows: Vec<Vec<u8>> = (0..height)
            .map(|row| {
                (0..width)
                    .map(|x| ((x * 3 + frame * 7 + row) % 200 + 20) as u8)
                    .collect()
            })
            .collect();
        for field in 0..2 {
            let n = frame * 2 + field;
            let mut set = |line: usize, values: Vec<u8>| rows[2 * line + field] = values;
            if n % 3 != 0 {
                let mut white = vec![235u8; width];
                white[..40].fill(16);
                set(11, white);
            }
            let picture = 0xf00000 | u32::from_str_radix(&(n + 1).to_string(), 16).unwrap();
            let clock = 26.0 + (n % 4) as f64 * 0.37;
            let start = 30.0 + (n % 5) as f64 * 3.0;
            let line16 = if n == 0 { 0x88ffff } else { 0x80dddd };
            set(16, code_line(width, line16, clock, start, &mut seed));
            let (line17, line18) = match n % 4 {
                1 => (picture, picture | 0xa),
                2 => (picture | 0xa0, picture),
                3 => (picture, 0x87ffff),
                _ => (picture, picture),
            };
            set(17, code_line(width, line17, clock, start, &mut seed));
            set(
                18,
                code_line(width, line18, clock + 0.2, start + 1.0, &mut seed),
            );
            if n == 5 {
                set(17, vec![128; width]);
            }
        }
        for (row, values) in rows.iter().enumerate() {
            for x in (0..width).step_by(2) {
                out.extend_from_slice(&[
                    values[x],
                    ((x + row) % 256) as u8,
                    values[x + 1],
                    ((row * 3) % 256) as u8,
                ]);
            }
        }
    }
    out
}

/// The fixtures: a name and the ffmpeg arguments making its AVI.
fn fixtures() -> Vec<(&'static str, Vec<String>)> {
    let lavfi =
        |video: &str, audio: &str, channels: &str, pix_fmt: &str, codec: &str, sample: &str| {
            [
                "-f", "lavfi", "-i", video, "-f", "lavfi", "-i", audio, "-ac", channels, "-c:v",
                codec, "-pix_fmt", pix_fmt, "-c:a", sample,
            ]
            .map(str::to_owned)
            .to_vec()
        };
    vec![
        (
            // interlaced NTSC, with the AVLD metadata of its VBI codes
            "ntsc",
            lavfi(
                "testsrc2=size=720x524:rate=30000/1001:duration=0.3",
                "sine=frequency=1000:sample_rate=44100:duration=1",
                "2",
                "yuyv422",
                "rawvideo",
                "pcm_s16le",
            ),
        ),
        (
            "pal",
            lavfi(
                "smptebars=size=720x624:rate=25:duration=0.3",
                "sine=frequency=300:sample_rate=48000:duration=1",
                "2",
                "yuyv422",
                "rawvideo",
                "pcm_s16le",
            ),
        ),
        (
            // progressive, at 60 frames a second, mono
            "progressive",
            lavfi(
                "testsrc=size=320x240:rate=60:duration=0.3",
                "sine=frequency=500:sample_rate=44100:duration=1",
                "1",
                "yuyv422",
                "rawvideo",
                "pcm_s16le",
            ),
        ),
        (
            "8-bit audio",
            lavfi(
                "testsrc2=size=352x288:rate=25:duration=0.3",
                "sine=frequency=700:sample_rate=22050:duration=1",
                "2",
                "yuyv422",
                "rawvideo",
                "pcm_u8",
            ),
        ),
        (
            "uyvy",
            lavfi(
                "testsrc2=size=720x480:rate=30000/1001:duration=0.3",
                "sine=frequency=440:sample_rate=48000:duration=1",
                "2",
                "uyvy422",
                "rawvideo",
                "pcm_s16le",
            ),
        ),
        (
            "huffyuv",
            [
                lavfi(
                    "testsrc2=size=720x524:rate=30000/1001:duration=0.3",
                    "sine=frequency=440:sample_rate=48000:duration=1",
                    "2",
                    "yuv422p",
                    "huffyuv",
                    "pcm_s16le",
                ),
                vec!["-pred".to_owned(), "left".to_owned()],
            ]
            .concat(),
        ),
        (
            "vbi",
            [
                "-f",
                "rawvideo",
                "-pix_fmt",
                "yuyv422",
                "-s",
                "720x524",
                "-r",
                "30000/1001",
                "-i",
                "frames.yuv",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=1000:sample_rate=44100:duration=1",
                "-shortest",
                "-ac",
                "2",
                "-c:v",
                "rawvideo",
                "-pix_fmt",
                "yuyv422",
                "-c:a",
                "pcm_s16le",
            ]
            .map(str::to_owned)
            .to_vec(),
        ),
    ]
}

#[test]
fn laserdiscs_match_chdman() {
    let (Some(chdman), Some(ffmpeg)) = (binary("chdman", "CHDMAN"), binary("ffmpeg", "FFMPEG"))
    else {
        eprintln!("skipping: chdman or ffmpeg not found");
        return;
    };
    let chdrs = PathBuf::from(env!("CARGO_BIN_EXE_chdrs"));
    for (name, args) in fixtures() {
        let dir = TempDir::new().unwrap();
        if name == "vbi" {
            std::fs::write(dir.path().join("frames.yuv"), vbi_frames(6)).unwrap();
        }
        let mut ffmpeg_args: Vec<&str> = vec!["-loglevel", "error", "-y"];
        ffmpeg_args.extend(args.iter().map(String::as_str));
        ffmpeg_args.push("disc.avi");
        run(&ffmpeg, dir.path(), &ffmpeg_args);

        run(
            &chdman,
            dir.path(),
            &["createld", "-i", "disc.avi", "-o", "theirs.chd"],
        );
        run(&chdrs, dir.path(), &["disc.avi"]);
        assert!(
            std::fs::read(dir.path().join("disc.chd")).unwrap()
                == std::fs::read(dir.path().join("theirs.chd")).unwrap(),
            "{name}: the CHD differs from chdman's"
        );

        run(
            &chdman,
            dir.path(),
            &["extractld", "-i", "theirs.chd", "-o", "theirs.avi"],
        );
        let mut chd = chd_rs::Chd::open(dir.path().join("theirs.chd")).unwrap();
        let ours = dir.path().join("ours.avi");
        let mut seen = 0;
        chd_rs::extract_ld(&mut chd, &ours, &mut |bytes| seen += bytes).unwrap();
        assert_eq!(seen, chd.info().logical_size, "{name}: progress");
        assert!(
            std::fs::read(&ours).unwrap() == std::fs::read(dir.path().join("theirs.avi")).unwrap(),
            "{name}: the AVI differs from chdman's"
        );
    }
}

/// A capture past 2 GiB, which ffmpeg writes as OpenDML and which extracts
/// back to an AVI split in `RIFF AVIX` parts with super indexes. Slow, and
/// some 7 GiB of disk: `cargo test --release -p chd-rs -- --ignored`.
#[test]
#[ignore]
fn large_laserdiscs_match_chdman() {
    let (Some(chdman), Some(ffmpeg)) = (binary("chdman", "CHDMAN"), binary("ffmpeg", "FFMPEG"))
    else {
        eprintln!("skipping: chdman or ffmpeg not found");
        return;
    };
    let chdrs = PathBuf::from(env!("CARGO_BIN_EXE_chdrs"));
    // next to the build, not in a temporary directory that may be in memory
    let dir = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    run(
        &ffmpeg,
        dir.path(),
        &[
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=720x524:rate=30000/1001:duration=100",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=101",
            "-ac",
            "2",
            "-c:v",
            "rawvideo",
            "-pix_fmt",
            "yuyv422",
            "-c:a",
            "pcm_s16le",
            "disc.avi",
        ],
    );
    run(
        &chdman,
        dir.path(),
        &["createld", "-i", "disc.avi", "-o", "theirs.chd"],
    );
    run(&chdrs, dir.path(), &["disc.avi"]);
    assert!(
        std::fs::read(dir.path().join("disc.chd")).unwrap()
            == std::fs::read(dir.path().join("theirs.chd")).unwrap(),
        "the CHD differs from chdman's"
    );
    std::fs::remove_file(dir.path().join("disc.avi")).unwrap();
    run(
        &chdman,
        dir.path(),
        &["extractld", "-i", "theirs.chd", "-o", "theirs.avi"],
    );
    let mut chd = chd_rs::Chd::open(dir.path().join("theirs.chd")).unwrap();
    chd_rs::extract_ld(&mut chd, &dir.path().join("ours.avi"), &mut |_| {}).unwrap();
    assert!(
        std::fs::read(dir.path().join("ours.avi")).unwrap()
            == std::fs::read(dir.path().join("theirs.avi")).unwrap(),
        "the AVI differs from chdman's"
    );
}

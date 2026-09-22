//! Shared fixture builders and the gdidrop reference, for the integration
//! tests. Each test crate uses a subset, so nothing here is dead on its own.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

pub const SECTOR: usize = 2352;

/// One track of a synthetic CUE/BIN set.
#[derive(Clone, Copy)]
pub struct Spec {
    pub audio: bool,
    /// Frames between INDEX 00 and INDEX 01; 0 for a lone INDEX 01.
    pub pregap: u64,
    /// Frames from INDEX 01 to the end of the BIN.
    pub sectors: u64,
    /// A `REM` written after this track, before the next one's FILE.
    pub rem: Option<&'static str>,
}

pub const fn data(pregap: u64, sectors: u64) -> Spec {
    Spec {
        audio: false,
        pregap,
        sectors,
        rem: None,
    }
}

pub const fn audio(pregap: u64, sectors: u64) -> Spec {
    Spec {
        audio: true,
        pregap,
        sectors,
        rem: None,
    }
}

/// A GD-ROM as Redump lays it out: two single-density tracks, then
/// `REM HIGH-DENSITY AREA` and the high-density tracks, the audio ones with
/// two-second pregaps and the last data track with a three-second one.
pub const GD_ROM: [Spec; 5] = [
    data(0, 300),
    Spec {
        rem: Some("HIGH-DENSITY AREA"),
        ..audio(150, 300)
    },
    data(0, 400),
    audio(150, 200),
    data(225, 300),
];

/// A plain CD: a data track with a pregap, then an audio track.
pub const CD: [Spec; 2] = [data(231, 400), audio(0, 200)];

/// A GD-ROM whose comment only looks like the high-density marker: gdidrop
/// matches it exactly, so this one must not jump to sector 45000.
pub const NEAR_MISS: [Spec; 3] = [
    data(0, 300),
    Spec {
        rem: Some("HIGH-DENSITY AREA (TOC)"),
        ..audio(150, 300)
    },
    data(0, 400),
];

/// Byte `offset` of sector `sector` of track `track`: every sector differs, so
/// a pregap dropped by the wrong amount shows.
pub fn byte(track: usize, sector: u64, offset: usize) -> u8 {
    ((track as u64 * 7 + sector * 13 + offset as u64) % 251) as u8
}

/// Write `specs` as `<stem>.cue` and one `<stem> (Track N).bin` each in `dir`,
/// with Redump's CRLF line endings, returning the CUE's path.
pub fn write_set(dir: &Path, stem: &str, specs: &[Spec]) -> PathBuf {
    let mut cue = String::from("REM SINGLE-DENSITY AREA\r\n");
    for (i, spec) in specs.iter().enumerate() {
        let bin = format!("{stem} (Track {}).bin", i + 1);
        let frames = spec.pregap + spec.sectors;
        let data: Vec<u8> = (0..frames)
            .flat_map(|sector| (0..SECTOR).map(move |offset| byte(i, sector, offset)))
            .collect();
        std::fs::write(dir.join(&bin), data).unwrap();

        cue.push_str(&format!("FILE \"{bin}\" BINARY\r\n"));
        let mode = if spec.audio { "AUDIO" } else { "MODE1/2352" };
        cue.push_str(&format!("  TRACK {:02} {mode}\r\n", i + 1));
        if spec.pregap > 0 {
            cue.push_str("    INDEX 00 00:00:00\r\n");
        }
        cue.push_str(&format!("    INDEX 01 {}\r\n", msf(spec.pregap)));
        if let Some(rem) = spec.rem {
            cue.push_str(&format!("REM {rem}\r\n"));
        }
    }
    let path = dir.join(format!("{stem}.cue"));
    std::fs::write(&path, cue).unwrap();
    path
}

/// The same disc as [`write_set`], with every track in one `<stem>.bin`: the
/// per-track BINs end to end, and the CUE's INDEX times counted from its start.
pub fn write_single(dir: &Path, stem: &str, specs: &[Spec]) -> PathBuf {
    let bin = format!("{stem}.bin");
    let mut data = Vec::new();
    let mut cue = format!("REM SINGLE-DENSITY AREA\r\nFILE \"{bin}\" BINARY\r\n");
    for (i, spec) in specs.iter().enumerate() {
        let offset = (data.len() / SECTOR) as u64;
        data.extend(
            (0..spec.pregap + spec.sectors)
                .flat_map(|sector| (0..SECTOR).map(move |offset| byte(i, sector, offset))),
        );
        let mode = if spec.audio { "AUDIO" } else { "MODE1/2352" };
        cue.push_str(&format!("  TRACK {:02} {mode}\r\n", i + 1));
        if spec.pregap > 0 {
            cue.push_str(&format!("    INDEX 00 {}\r\n", msf(offset)));
        }
        cue.push_str(&format!("    INDEX 01 {}\r\n", msf(offset + spec.pregap)));
        if let Some(rem) = spec.rem {
            cue.push_str(&format!("REM {rem}\r\n"));
        }
    }
    std::fs::write(dir.join(&bin), data).unwrap();
    let path = dir.join(format!("{stem}.cue"));
    std::fs::write(&path, cue).unwrap();
    path
}

fn msf(frames: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        frames / 75 / 60,
        frames / 75 % 60,
        frames % 75
    )
}

/// gdidrop's conversion, built once from `tests/reference` against a gdidrop
/// checkout, or `None` when dotnet or the checkout is missing. The checkout is
/// `$GDIDROP_SOURCE`, or `gdidrop-Dreamcast-Redump-Tool` next to the oxyromon
/// repository.
pub fn reference() -> Option<&'static Path> {
    static REFERENCE: OnceLock<Option<PathBuf>> = OnceLock::new();
    REFERENCE.get_or_init(build_reference).as_deref()
}

fn build_reference() -> Option<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = std::env::var_os("GDIDROP_SOURCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest.join("../../../gdidrop-Dreamcast-Redump-Tool"));
    if !source.join("Source/gdidrop/CueSharp.cs").is_file() {
        return None;
    }
    Command::new("dotnet").arg("--version").output().ok()?;

    // Built outside the source tree, so no obj/ lands in the crate.
    let project = Path::new(env!("CARGO_TARGET_TMPDIR")).join("gdidrop-reference");
    let out = project.join("out");
    std::fs::create_dir_all(&project).unwrap();
    for file in ["GdidropReference.csproj", "Program.cs"] {
        std::fs::copy(
            manifest.join("tests/reference").join(file),
            project.join(file),
        )
        .unwrap();
    }
    let build = Command::new("dotnet")
        .current_dir(&project)
        .args(["build", "-c", "Release", "-o"])
        .arg(&out)
        .arg(format!(
            "-p:GdidropSource={}",
            source.canonicalize().unwrap().display()
        ))
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "building the gdidrop reference failed:\n{}",
        String::from_utf8_lossy(&build.stdout)
    );
    Some(out.join("GdidropReference.dll"))
}

/// Run gdidrop on `cue`: it writes the GDI and `<stem> [gdidrop].bin|raw`
/// tracks next to it.
pub fn run_reference(reference: &Path, cue: &Path) {
    let run = Command::new("dotnet")
        .arg(reference)
        .arg(cue)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "gdidrop failed on {}: {}",
        cue.display(),
        String::from_utf8_lossy(&run.stderr)
    );
}

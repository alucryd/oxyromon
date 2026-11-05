//! Agreement with Dolphin: our RVZs are byte for byte dolphin-tool's,
//! scrubbed or not, wherever the codec allows, and each reads the other's
//! back to the same disc where it does not.
//!
//! Skipped unless dolphin-tool is reachable through `$DOLPHIN_TOOL` or
//! `$PATH`. The synthetic Wii disc is encrypted with a made-up common key,
//! which dolphin-tool does not know: it reads our RVZs of it, which keep the
//! title key, but rewriting one it decrypts the ticket with the real common
//! key instead, so only a real disc, in the ignored test below, shows its
//! Wii RVZs are ours byte for byte.

use std::path::{Path, PathBuf};
use std::process::Command;

use rvz_rs::{Compression, Options};
use tempfile::TempDir;

mod common;
use common::{COMMON_KEY, dolphin_tool, gamecube_disc, wii_disc, write};

fn tool() -> Option<PathBuf> {
    let tool = dolphin_tool();
    if tool.is_none() {
        eprintln!("skipping: no dolphin-tool found (set $DOLPHIN_TOOL)");
    }
    tool
}

/// Convert with dolphin-tool, `options` naming an RVZ's settings.
fn dolphin(tool: &Path, input: &Path, output: &Path, options: Option<&Options>) {
    let mut command = Command::new(tool);
    command.args(["convert", "-i"]).arg(input).arg("-o").arg(output);
    match options {
        None => {
            command.args(["-f", "iso"]);
        }
        Some(options) => {
            command
                .args(["-f", "rvz", "-c", options.compression_name(), "-l"])
                .arg(options.level.to_string())
                .arg("-b")
                .arg(options.chunk_size.to_string());
            if options.scrub {
                command.arg("-s");
            }
        }
    }
    let out = command.output().expect("failed to run dolphin-tool");
    assert!(
        out.status.success(),
        "dolphin-tool failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

trait Name {
    fn compression_name(&self) -> &'static str;
}

impl Name for Options {
    fn compression_name(&self) -> &'static str {
        match self.compression {
            Compression::None => "none",
            Compression::Bzip2 => "bzip2",
            Compression::Lzma => "lzma",
            Compression::Lzma2 => "lzma2",
            Compression::Zstd => "zstd",
        }
    }
}

fn options(compression: Compression, level: i32, chunk_size: u32, scrub: bool) -> Options {
    Options {
        compression,
        level,
        chunk_size,
        scrub,
    }
}

/// The settings both tools write the same bytes with.
fn same_bytes() -> Vec<Options> {
    vec![
        options(Compression::Zstd, 5, 0x20000, false),
        options(Compression::Zstd, 19, 0x8000, false),
        options(Compression::None, 0, 0x20000, false),
        options(Compression::Bzip2, 9, 0x200000, false),
        options(Compression::Zstd, 3, 0x400000, false),
        options(Compression::Zstd, 5, 0x20000, true),
        options(Compression::None, 0, 0x20000, true),
    ]
}

fn assert_same_file(ours: &Path, theirs: &Path, what: &str) {
    let ours = std::fs::read(ours).unwrap();
    let theirs = std::fs::read(theirs).unwrap();
    assert!(
        ours == theirs,
        "{what}: {} bytes against dolphin-tool's {}",
        ours.len(),
        theirs.len()
    );
}

#[test]
fn a_gamecube_rvz_is_dolphins_byte_for_byte() {
    let Some(tool) = tool() else { return };
    let dir = TempDir::new().unwrap();
    let iso = dir.path().join("disc.iso");
    write(&iso, &gamecube_disc(0x580000));
    for options in same_bytes() {
        let ours = dir.path().join("ours.rvz");
        let theirs = dir.path().join("theirs.rvz");
        rvz_rs::compress(&iso, &ours, &options, &[], &mut |_| {}).unwrap();
        dolphin(&tool, &iso, &theirs, Some(&options));
        assert_same_file(&ours, &theirs, &format!("{options:?}"));
    }
}

#[test]
fn dolphin_reads_our_wii_rvzs() {
    let Some(tool) = tool() else { return };
    let dir = TempDir::new().unwrap();
    let disc = wii_disc();
    let iso = dir.path().join("disc.iso");
    let rvz = dir.path().join("disc.rvz");
    let out = dir.path().join("out.iso");
    write(&iso, &disc);
    rvz_rs::compress(&iso, &rvz, &Options::default(), &[COMMON_KEY], &mut |_| {}).unwrap();
    // dolphin-tool reads it back to the disc, with the title key it keeps
    dolphin(&tool, &rvz, &out, None);
    assert!(std::fs::read(&out).unwrap() == disc, "dolphin-tool reads our Wii RVZ wrong");
    // Every way we write it
    for options in same_bytes().into_iter().filter(|options| !options.scrub) {
        let ours = dir.path().join("ours.rvz");
        rvz_rs::compress(&rvz, &ours, &options, &[], &mut |_| {}).unwrap();
        dolphin(&tool, &ours, &out, None);
        assert!(std::fs::read(&out).unwrap() == disc, "dolphin-tool reads our {options:?} wrong");
    }
}

#[test]
fn each_reads_the_others_lzma() {
    let Some(tool) = tool() else { return };
    let dir = TempDir::new().unwrap();
    let disc = gamecube_disc(0x580000);
    let iso = dir.path().join("disc.iso");
    write(&iso, &disc);
    for compression in [Compression::Lzma, Compression::Lzma2] {
        let options = options(compression, 6, 0x20000, false);
        let ours = dir.path().join("ours.rvz");
        let theirs = dir.path().join("theirs.rvz");
        let out = dir.path().join("out.iso");
        rvz_rs::compress(&iso, &ours, &options, &[], &mut |_| {}).unwrap();
        dolphin(&tool, &ours, &out, None);
        assert!(std::fs::read(&out).unwrap() == disc, "dolphin-tool reads our {compression:?} wrong");
        dolphin(&tool, &iso, &theirs, Some(&options));
        rvz_rs::decompress(&theirs, &out, &mut |_| {}).unwrap();
        assert!(std::fs::read(&out).unwrap() == disc, "we read dolphin-tool's {compression:?} wrong");
    }
}

/// A real RVZ, from `$RVZ_RS_REAL`: we read it as dolphin-tool does, and
/// rewrite it, scrubbed or not, as dolphin-tool does. Copy the RVZ to fast
/// local storage first: this reads and writes it several times.
#[test]
#[ignore = "reads and writes a real disc several times over"]
fn a_real_rvz_is_read_and_rewritten_as_dolphin_does() {
    let Some(tool) = tool() else { return };
    let Some(rvz) = std::env::var_os("RVZ_RS_REAL").map(PathBuf::from) else {
        eprintln!("skipping: set $RVZ_RS_REAL to an RVZ");
        return;
    };
    let dir = TempDir::new_in(rvz.parent().unwrap()).unwrap();
    let ours = dir.path().join("ours.iso");
    let theirs = dir.path().join("theirs.iso");
    rvz_rs::decompress(&rvz, &ours, &mut |_| {}).unwrap();
    dolphin(&tool, &rvz, &theirs, None);
    assert_same_file(&ours, &theirs, "decompressed");
    std::fs::remove_file(&ours).unwrap();
    std::fs::remove_file(&theirs).unwrap();
    for options in [
        options(Compression::Zstd, 5, 0x20000, false),
        options(Compression::Zstd, 5, 0x20000, true),
    ] {
        let ours = dir.path().join("ours.rvz");
        let theirs = dir.path().join("theirs.rvz");
        rvz_rs::compress(&rvz, &ours, &options, &[], &mut |_| {}).unwrap();
        dolphin(&tool, &rvz, &theirs, Some(&options));
        assert_same_file(&ours, &theirs, &format!("{options:?}"));
    }
}

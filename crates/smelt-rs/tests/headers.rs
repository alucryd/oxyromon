//! `smelt.h` is cbindgen's from the crate as it is, compiles as C99 and with
//! `smelt.hpp` as C++11, and a C program linked against libsmelt reads an
//! ORE through it. Each step skips, rather than fails, when its tool is
//! missing.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

fn crate_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn have(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
}

#[test]
fn the_header_is_cbindgens() {
    if !have("cbindgen") {
        eprintln!("cbindgen not found, skipping");
        return;
    }
    let dir = TempDir::new().unwrap();
    let header = dir.path().join("smelt.h");
    let status = Command::new("cbindgen")
        .current_dir(crate_dir())
        .args(["--config", "cbindgen.toml", "--output"])
        .arg(&header)
        .status()
        .unwrap();
    assert!(status.success());
    assert!(
        std::fs::read(&header).unwrap()
            == std::fs::read(crate_dir().join("include/smelt.h")).unwrap(),
        "include/smelt.h is stale: cbindgen --config cbindgen.toml --output include/smelt.h"
    );
}

#[test]
fn the_headers_compile_as_c_and_cpp() {
    let include = crate_dir().join("include");
    for (compiler, language, standard, header) in [
        ("cc", "c", "-std=c99", "smelt.h"),
        ("c++", "c++", "-std=c++11", "smelt.hpp"),
    ] {
        if !have(compiler) {
            eprintln!("{compiler} not found, skipping");
            continue;
        }
        let output = Command::new(compiler)
            .args([
                standard,
                "-Wall",
                "-Wextra",
                "-pedantic",
                "-Werror",
                "-fsyntax-only",
                "-x",
                language,
            ])
            .arg(include.join(header))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{header}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// libsmelt.so as the crate is now: `cargo test` builds the rlib alone, so
/// the shared library is built here, cargo naming the file it wrote.
fn shared_library() -> Option<PathBuf> {
    let output = Command::new(env!("CARGO"))
        .current_dir(crate_dir())
        .args(["build", "--lib", "--message-format=json"])
        .output()
        .unwrap();
    if !output.status.success() {
        panic!("{}", String::from_utf8_lossy(&output.stderr));
    }
    // shortcut: the JSON scanned for the path rather than parsed, no JSON
    // crate being a dependency; parse it if a cargo release breaks the scan
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.contains("\"compiler-artifact\"") && line.contains("\"cdylib\""))
        .flat_map(|line| line.split('"').map(str::to_owned).collect::<Vec<_>>())
        .find(|field| field.ends_with("/libsmelt.so"))
        .map(PathBuf::from)
}

#[test]
fn a_c_program_reads_an_ore() {
    if !cfg!(target_os = "linux") || !have("cc") {
        eprintln!("not Linux, or cc not found, skipping");
        return;
    }
    let library = shared_library().expect("cargo named no libsmelt.so");
    let dir = TempDir::new().unwrap();
    let rom: Vec<u8> = (0..40_000u32).map(|i| (i * 7 % 251) as u8).collect();
    let input = dir.path().join("Game.nes");
    std::fs::write(&input, &rom).unwrap();
    let ore = dir.path().join("game.ore");
    let options = ore_rs::Options {
        platform: Some("nes"),
        ..ore_rs::Options::default()
    };
    ore_rs::compress(&input, &ore, &options, &[], &mut |_| {}).unwrap();

    let program = dir.path().join("read");
    let status = Command::new("cc")
        .args(["-std=c99", "-Wall", "-Wextra", "-pedantic", "-Werror", "-o"])
        .arg(&program)
        .arg(crate_dir().join("tests/read.c"))
        .arg("-I")
        .arg(crate_dir().join("include"))
        .arg(&library)
        .status()
        .unwrap();
    assert!(status.success());
    let output = Command::new(&program)
        .arg(&ore)
        .env("LD_LIBRARY_PATH", library.parent().unwrap())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout == rom);
}

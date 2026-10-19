//! Agreement with the reference implementation: `ctrtool -p` and this crate
//! extract the same bytes for every part of the container. This is the test
//! that matters — a CIA importer is only right if the hashes it produces are
//! the ones DATs recorded against ctrtool's output.
//!
//! Skipped unless a ctrtool binary is reachable through `$CTRTOOL` or
//! `$PATH`.

use std::path::Path;
use std::process::Command;

use cia_rs::UnpackOptions;

mod common;
use common::{ctrtool_binary, fixture};

#[test]
fn plain_extraction_matches_ctrtool() {
    let Some(ctrtool) = ctrtool_binary() else {
        eprintln!("skipping: no ctrtool binary found (set $CTRTOOL)");
        return;
    };
    let input = fixture();
    let dirs = tempfile::tempdir().unwrap();
    let ours = dirs.path().join("ours");
    let theirs = dirs.path().join("theirs");
    std::fs::create_dir_all(&theirs).unwrap();

    cia_rs::unpack(&input, &ours, &UnpackOptions::default(), &mut |_| {}).unwrap();

    // The reference names each part the file it is given, contents included:
    // `--contents=PATH` gets `.<index>.<id>` appended to it. `--footer` is
    // advertised but rejected by the 1.3.0 build, so the footer is the one
    // part this does not cross-check. Its exit code says little — plain mode
    // on a CIA it can half-read still writes the parts, which is all either
    // side promises.
    let _ = Command::new(&ctrtool)
        .args(["-p", "-q"])
        .arg("--certs")
        .arg(theirs.join("certs"))
        .arg("--tik")
        .arg(theirs.join("tik"))
        .arg("--tmd")
        .arg(theirs.join("tmd"))
        .arg("--contents")
        .arg(theirs.join("content"))
        .arg(&input)
        .output();

    for name in ["certs", "tik", "tmd"] {
        let mine = read(&ours.join(name));
        let reference = read(&theirs.join(name));
        assert_eq!(mine, reference, "{name} differs");
    }

    let mut mine = contents(&ours);
    let mut reference = contents(&theirs);
    mine.sort();
    reference.sort();
    assert_eq!(
        mine.len(),
        reference.len(),
        "content count differs: {:?} vs {:?}",
        names(&mine),
        names(&reference)
    );
    for ((our_name, our_bytes), (their_name, their_bytes)) in mine.iter().zip(&reference) {
        assert_eq!(
            our_bytes, their_bytes,
            "contents differ ({our_name} vs {their_name})"
        );
    }
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path)
        .unwrap_or_else(|e| panic!("{}: {e} — ctrtool or we failed to write it", path.display()))
}

/// The files of a directory named `content. …`, as pairs of name and bytes.
fn contents(dir: &Path) -> Vec<(String, Vec<u8>)> {
    std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().unwrap().is_file())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with("content.")
                .then(|| (name, std::fs::read(entry.path()).unwrap()))
        })
        .collect()
}

fn names(files: &[(String, Vec<u8>)]) -> Vec<&str> {
    files.iter().map(|(name, _)| name.as_str()).collect()
}

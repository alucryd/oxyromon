//! Against the reference: patches the installed xdelta3 writes now, across its
//! window sizes, levels and secondary compressors. Skipped when it is missing.

use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn xdelta3() -> bool {
    Command::new("xdelta3").arg("-V").output().is_ok()
}

/// A few MiB of ROM-like data, and a changed copy: overwrites, an insertion, a
/// deletion, and a tail copied from elsewhere.
fn files(directory: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut random = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let words: [&[u8]; 5] = [b"SPRITE", b"LEVEL", b"\0\0\0\0", b"\xff\xff", b"MUSIC"];
    let mut source = Vec::new();
    while source.len() < 3 << 20 {
        if random() % 4 == 0 {
            source.extend((0..random() % 32).map(|_| random() as u8));
        } else {
            let word = words[(random() % 5) as usize];
            for _ in 0..random() % 8 {
                source.extend_from_slice(word);
            }
        }
    }
    let mut target = source.clone();
    for i in (0..target.len()).step_by(100_000) {
        target[i] = random() as u8;
    }
    target.splice(500_000..500_000, b"INSERTED".repeat(5_000));
    target.drain(1_500_000..1_700_000);
    target.extend_from_slice(&source[1_000..300_000]);

    let (source_path, target_path) = (directory.join("source.bin"), directory.join("target.bin"));
    std::fs::write(&source_path, source).unwrap();
    std::fs::write(&target_path, target).unwrap();
    (source_path, target_path)
}

#[test]
fn every_xdelta3_configuration_decodes() {
    if !xdelta3() {
        eprintln!("xdelta3 not installed, skipping");
        return;
    }
    let directory = TempDir::new().unwrap();
    let (source, target) = files(directory.path());
    let configurations: [&[&str]; 7] = [
        &["-S", "lzma"],
        &["-S", "none"],
        &["-9", "-S", "lzma"],
        &["-0", "-S", "lzma"],
        &["-S", "lzma", "-W", "16384"],
        &["-S", "lzma", "-B", "1048576"],
        &["-S", "djw"],
    ];
    for (i, arguments) in configurations.iter().enumerate() {
        let patch = directory.path().join(format!("{i}.xdelta"));
        let status = Command::new("xdelta3")
            .args(["-e", "-f"])
            .args(*arguments)
            .arg("-s")
            .arg(&source)
            .arg(&target)
            .arg(&patch)
            .status()
            .unwrap();
        assert!(status.success());
        let output = directory.path().join(format!("{i}.out"));
        match xdelta_rs::decode(Some(&source), &patch, &output, &mut |_| {}) {
            Ok(()) => assert!(
                std::fs::read(&output).unwrap() == std::fs::read(&target).unwrap(),
                "{arguments:?}"
            ),
            // DJW is refused only once it has compressed a section.
            Err(xdelta_rs::Error::Unsupported(message)) if arguments.contains(&"djw") => {
                assert!(message.contains("DJW"));
            }
            Err(error) => panic!("{arguments:?}: {error}"),
        }
    }
}

#[test]
fn a_patch_without_a_source_decodes() {
    if !xdelta3() {
        return;
    }
    let directory = TempDir::new().unwrap();
    let (_, target) = files(directory.path());
    let patch = directory.path().join("nosource.xdelta");
    assert!(
        Command::new("xdelta3")
            .args(["-e", "-f", "-S", "lzma"])
            .arg(&target)
            .arg(&patch)
            .status()
            .unwrap()
            .success()
    );
    let output = directory.path().join("out.bin");
    xdelta_rs::decode(None, &patch, &output, &mut |_| {}).unwrap();
    assert!(std::fs::read(&output).unwrap() == std::fs::read(&target).unwrap());
}

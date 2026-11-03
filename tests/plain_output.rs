// The CLI's output when stderr is not a terminal, as in a pipe, a log file or
// a service: indicatif hides its bars there, and prints nothing above them.

use std::process::Command;
use tempfile::TempDir;

#[test]
fn a_pipe_gets_the_lines_without_colors() {
    let directory = TempDir::new().expect("temp directory");
    let output = Command::new(env!("CARGO_BIN_EXE_oxyromon"))
        .args(["import-dats", "tests/Test System (20200721).dat"])
        .env("OXYROMON_DATA_DIRECTORY", directory.path())
        .env("OXYROMON_ROM_DIRECTORY", directory.path().join("roms"))
        .env("OXYROMON_TMP_DIRECTORY", directory.path())
        .env("OXYROMON_LOG_LEVEL", "off")
        .output()
        .expect("run oxyromon import-dats");
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 on stderr");

    assert!(output.status.success(), "{stderr}");
    assert!(output.stdout.is_empty(), "stdout: {:?}", output.stdout);
    for line in [
        "◆ Processing \"Test System (20200721).dat\"",
        "ℹ System: Test System",
        "ℹ Version: 20200721",
    ] {
        assert!(stderr.contains(line), "{line:?} missing from:\n{stderr}");
    }
    assert!(!stderr.contains('\u{1b}'), "escape codes in:\n{stderr}");
}

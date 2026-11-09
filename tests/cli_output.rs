// Scripts and log files read a command's output through a pipe, where the
// progress bar hides itself.

use std::process::Command;

#[test]
fn messages_reach_a_stderr_that_is_not_a_terminal() {
    let directory = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_oxyromon"))
        .arg("info")
        .env("OXYROMON_DATA_DIRECTORY", directory.path())
        .env("OXYROMON_ROM_DIRECTORY", directory.path())
        .env("OXYROMON_TMP_DIRECTORY", directory.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("oxyROMon"));
    assert!(output.stdout.is_empty());
}

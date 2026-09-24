//! What every oxyROMon tool looks like from the command line: oxyROMon's own
//! progress bar and result lines, and the loop around them. This file is the
//! same in each tool's CLI; change them all together.

// Each tool uses a subset.
#![allow(dead_code)]

use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use console::Style;
use indicatif::{ProgressBar, ProgressStyle};

/// Under every tool's `--help`.
pub const AFTER_HELP: &str =
    "Part of oxyROMon, the Rusty ROM OrgaNizer: https://github.com/alucryd/oxyromon";

/// Convert each input in turn, reporting how each went, and fail if any did.
///
/// `convert` returns the line to report on success.
pub fn run_all<'a>(
    inputs: impl IntoIterator<Item = &'a PathBuf>,
    mut convert: impl FnMut(&Path) -> Result<String, String>,
) -> ExitCode {
    let mut failed = false;
    for input in inputs {
        match convert(input) {
            Ok(message) => success(message),
            Err(message) => {
                error(format!("{}: {message}", input.display()));
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Where to write what `input` converts to: `output` when given, next to
/// `input` otherwise. Created when missing.
pub fn output_dir(input: &Path, output: Option<&PathBuf>) -> Result<PathBuf, String> {
    let dir = match output {
        Some(output) => output.clone(),
        // Empty for a bare file name, which is the current directory.
        None => input.parent().unwrap_or(Path::new("")).to_path_buf(),
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

/// `dir/<input's stem>.<extension>`, keeping any dot in the stem.
pub fn output_path(dir: &Path, input: &Path, extension: &str) -> PathBuf {
    let stem = input.file_stem().unwrap_or_default().to_string_lossy();
    dir.join(format!("{stem}.{extension}"))
}

/// `input`'s extension, lowercased.
pub fn extension(input: &Path) -> String {
    input
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase()
}

/// A bar for one file, `length` bytes long, drawn as oxyROMon draws its own.
/// Hidden when stderr isn't a terminal.
pub fn progress_bar(length: u64, verb: &str, input: &Path) -> ProgressBar {
    let bar = ProgressBar::new(length).with_style(
        ProgressStyle::with_template(
            "  {spinner:.cyan} {wide_msg}\n        {bytes}/{total_bytes} [{bar:40.cyan/dim}] {bytes_per_sec} {elapsed_precise} (ETA {eta_precise})",
        )
        .expect("a valid template")
        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
        .progress_chars("━╸━"),
    );
    let name = input.file_name().unwrap_or_default().to_string_lossy();
    bar.set_message(format!("{verb} \"{name}\""));
    bar.enable_steady_tick(Duration::from_millis(100));
    bar
}

/// The success line for a written file.
pub fn wrote(path: &Path) -> String {
    format!("Wrote \"{}\"", path.display())
}

fn success(message: impl Display) {
    println!("    {} {message}", Style::new().green().apply_to("✔"));
}

fn error(message: impl Display) {
    eprintln!("    {} {message}", Style::new().red().bold().apply_to("✖"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_names_keep_their_dots() {
        let path = output_path(Path::new("out"), Path::new("in/Game (v1.02).iso"), "cso");
        assert_eq!(path, Path::new("out/Game (v1.02).cso"));
    }

    #[test]
    fn output_goes_next_to_the_input_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("game.iso");
        assert_eq!(output_dir(&input, None).unwrap(), dir.path());
        assert_eq!(
            output_dir(Path::new("game.iso"), None).unwrap(),
            Path::new("")
        );
    }
}

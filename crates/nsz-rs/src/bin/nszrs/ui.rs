//! What every oxyROMon tool looks like from the command line: oxyROMon's own
//! progress bar and result lines, and the loop around them. This file is the
//! same in each tool's CLI, which a test in oxyromon checks; change them all
//! together.

// Each tool uses a subset.
#![allow(dead_code)]

use std::collections::HashSet;
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{ArgMatches, Command};
use console::Style;
use indicatif::{ProgressBar, ProgressStyle};

/// Under every tool's `--help`.
pub const AFTER_HELP: &str =
    "Part of oxyROMon, the Rusty ROM OrgaNizer: https://github.com/alucryd/oxyromon";

/// Run a tool: parse `command`'s arguments, then convert each of its `INPUTS`
/// in turn, reporting how each went, and fail if any did.
///
/// `convert` returns the line to report on success, and claims what it writes
/// from `outputs` first.
pub fn run(
    command: Command,
    mut convert: impl FnMut(&Path, &ArgMatches, &mut Outputs) -> Result<String, String>,
) -> ExitCode {
    let matches = command.get_matches();
    let inputs: Vec<&PathBuf> = matches.get_many::<PathBuf>("INPUTS").unwrap().collect();
    let mut outputs = Outputs::new(&inputs);
    let mut failed = false;
    for input in inputs {
        match convert(input, &matches, &mut outputs) {
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

/// What a run writes, so that no input's output lands on another's, nor on an
/// input: `-o out/ a/game.iso b/game.iso` would otherwise write `out/game.cso`
/// twice.
pub struct Outputs {
    inputs: HashSet<PathBuf>,
    written: HashSet<PathBuf>,
}

impl Outputs {
    fn new(inputs: &[&PathBuf]) -> Outputs {
        Outputs {
            inputs: inputs
                .iter()
                .filter_map(|input| std::path::absolute(input).ok())
                .collect(),
            written: HashSet::new(),
        }
    }

    /// Claim `path` for the input being converted, unless an input or an
    /// earlier output already is `path`.
    pub fn claim(&mut self, path: PathBuf) -> Result<PathBuf, String> {
        let absolute = std::path::absolute(&path).map_err(|e| e.to_string())?;
        if self.inputs.contains(&absolute) {
            return Err(format!(
                "would overwrite \"{}\", one of the inputs",
                path.display()
            ));
        }
        if !self.written.insert(absolute) {
            return Err(format!(
                "would overwrite \"{}\", written for an earlier input",
                path.display()
            ));
        }
        Ok(path)
    }
}

/// Where to write what `input` converts to: `-o` when given, next to `input`
/// otherwise. Created when missing, so call it once `input` is validated.
pub fn output_dir(input: &Path, matches: &ArgMatches) -> Result<PathBuf, String> {
    let dir = match matches.get_one::<PathBuf>("output") {
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
    use clap::Arg;

    fn matches(argv: &[&str]) -> ArgMatches {
        Command::new("tool")
            .arg(
                Arg::new("output")
                    .short('o')
                    .value_parser(clap::value_parser!(PathBuf)),
            )
            .try_get_matches_from(argv)
            .unwrap()
    }

    #[test]
    fn output_names_keep_their_dots() {
        let path = output_path(Path::new("out"), Path::new("in/Game (v1.02).iso"), "cso");
        assert_eq!(path, Path::new("out/Game (v1.02).cso"));
    }

    #[test]
    fn output_goes_next_to_the_input_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("game.iso");
        assert_eq!(output_dir(&input, &matches(&["tool"])).unwrap(), dir.path());
        assert_eq!(
            output_dir(Path::new("game.iso"), &matches(&["tool"])).unwrap(),
            Path::new("")
        );
    }

    #[test]
    fn no_output_is_written_twice_nor_over_an_input() {
        let (a, b) = (PathBuf::from("a/game.iso"), PathBuf::from("game.cso"));
        let mut outputs = Outputs::new(&[&a, &b]);
        outputs.claim(PathBuf::from("out/game.cso")).unwrap();
        assert!(outputs.claim(PathBuf::from("./out/game.cso")).is_err());
        assert!(outputs.claim(PathBuf::from("./game.cso")).is_err());
        assert!(outputs.claim(PathBuf::from("a/game.cso")).is_ok());
    }
}

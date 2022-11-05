use super::progress::*;
use super::SimpleResult;
use async_std::path::Path;
use indicatif::ProgressBar;
use lazy_static::lazy_static;
use regex::Regex;
use std::process::Command;
use std::time::Duration;

lazy_static! {
    static ref DIRECTORY_RE: Regex = Regex::new(r"^Directory listing of /(.+)$").unwrap();
    static ref FILE_RE: Regex = Regex::new(
        r"^-[rwx-]{9}\s+[0-9]\s+[0-9]\s+[0-9]\s+([0-9]+).*\[\s*([0-9]+) [0-9]{2}\] ([^;]+).*$"
    )
    .unwrap();
}

pub fn parse_iso<P: AsRef<Path>>(
    progress_bar: &ProgressBar,
    iso_path: &P,
) -> SimpleResult<Vec<(String, i64, u64)>> {
    progress_bar.set_message("Parsing ISO header");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    let output = Command::new("isoinfo")
        .arg("-i")
        .arg(iso_path.as_ref())
        .arg("-J")
        .arg("-l")
        .output()
        .expect("Failed to parse ISO header");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    let mut files: Vec<(String, i64, u64)> = Vec::new();
    let mut directory = "";

    for line in String::from_utf8(output.stdout).unwrap().lines() {
        if let Some(line_match) = DIRECTORY_RE.captures(line) {
            directory = line_match.get(1).unwrap().as_str();
        }
        if let Some(line_match) = FILE_RE.captures(line) {
            files.push((
                format!("{}{}", directory, line_match.get(3).unwrap().as_str()),
                line_match.get(1).unwrap().as_str().parse().unwrap(),
                line_match.get(2).unwrap().as_str().parse().unwrap(),
            ));
        }
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(files)
}

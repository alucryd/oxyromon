use super::progress::*;
use super::SimpleResult;
use cdfs::{DirectoryEntry, ExtraAttributes, ISO9660Reader, ISODirectory, ISOFile, ISO9660};
use indicatif::ProgressBar;
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashMap;
use std::fs::File;
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;

const ISOINFO: &str = "iso-info";

lazy_static! {
    static ref DIRECTORY_REGEX: Regex = Regex::new(r"^/(.+):$").unwrap();
    static ref FILE_REGEX: Regex =
        Regex::new(r"^\s+-\s+\[.*\s+(\d+)\]\s+(\d+)\s+.*\s+(.+)$").unwrap();
    static ref VERSION_REGEX: Regex = Regex::new(r"\d+\.\d+\.\d+").unwrap();
}

pub async fn get_version() -> SimpleResult<String> {
    let output = try_with!(
        Command::new(ISOINFO).arg("--version").output().await,
        "Failed to spawn iso-info"
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let version = stdout
        .lines()
        .next()
        .and_then(|line| VERSION_REGEX.find(line))
        .map(|version| version.as_str().to_string())
        .unwrap_or(String::from("unknown"));

    Ok(version)
}

pub async fn parse_iso<P: AsRef<Path>>(
    progress_bar: &ProgressBar,
    iso_path: &P,
) -> SimpleResult<HashMap<String, (i64, u64)>> {
    progress_bar.set_message("Parsing ISO header");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    let file = File::open(iso_path).unwrap();
    let iso = ISO9660::new(file).unwrap();
    for entry in iso.root().contents() {
        let mode = entry.as_ref().unwrap().mode();
        println!("{}", entry.unwrap().identifier());
    }

    let output = Command::new(ISOINFO)
        .arg("-i")
        .arg(iso_path.as_ref())
        .arg("-l")
        .arg("--no-header")
        .arg("--quiet")
        .output()
        .await
        .expect("Failed to parse ISO header");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    let mut files: HashMap<String, (i64, u64)> = HashMap::new();
    let mut directory = "";

    for line in String::from_utf8(output.stdout).unwrap().lines() {
        if let Some(line_match) = DIRECTORY_REGEX.captures(line) {
            directory = line_match.get(1).unwrap().as_str();
        }
        if let Some(line_match) = FILE_REGEX.captures(line) {
            let path = format!("{}{}", directory, line_match.get(3).unwrap().as_str());
            let size: i64 = line_match.get(2).unwrap().as_str().parse().unwrap();
            let start: u64 = line_match.get(1).unwrap().as_str().parse().unwrap();
            if let Some(file) = files.get(&path) {
                files.insert(path, (file.0 + size, file.1));
            } else {
                files.insert(path, (size, start));
            }
        }
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(files)
}

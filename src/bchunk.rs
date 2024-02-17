use super::config::*;
use super::progress::*;
use super::SimpleResult;
use indicatif::ProgressBar;
use lazy_static::lazy_static;
use regex::Regex;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;

const BCHUNK: &str = "bchunk";

lazy_static! {
    static ref VERSION_REGEX: Regex = Regex::new(r"\d+\.\d+\.\d+").unwrap();
}

pub async fn get_version() -> SimpleResult<String> {
    let output = try_with!(
        Command::new(BCHUNK).output().await,
        "Failed to spawn bchunk"
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

pub async fn create_iso<P: AsRef<Path>, Q: AsRef<Path>, R: AsRef<Path>>(
    progress_bar: &ProgressBar,
    bin_path: &P,
    cue_path: &Q,
    directory: &R,
) -> SimpleResult<PathBuf> {
    progress_bar.set_message("Creating ISO");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    let iso_path = directory
        .as_ref()
        .join(bin_path.as_ref().file_name().unwrap())
        .with_extension(ISO_EXTENSION);

    let output = Command::new(BCHUNK)
        .arg(bin_path.as_ref())
        .arg(cue_path.as_ref())
        .arg(&iso_path)
        .output()
        .await
        .expect("Failed to parse ISO header");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(iso_path)
}

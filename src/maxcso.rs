use super::config::*;
use super::progress::*;
use super::SimpleResult;
use indicatif::ProgressBar;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

const MAXCSO: &str = "maxcso";

lazy_static! {
    static ref VERSION_REGEX: Regex = Regex::new(r"\d+\.\d+\.\d+").unwrap();
}

pub async fn get_version() -> SimpleResult<String> {
    let output = try_with!(
        Command::new(MAXCSO).output().await,
        "Failed to spawn maxcso"
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    let version = stderr
        .lines()
        .next()
        .map(|line| VERSION_REGEX.find(line))
        .flatten()
        .map(|version| version.as_str().to_string())
        .unwrap_or(String::from("unknown"));

    Ok(version)
}

pub async fn create_cso<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    iso_path: &P,
    directory: &Q,
) -> SimpleResult<PathBuf> {
    progress_bar.set_message("Creating CSO");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    let mut cso_path = directory
        .as_ref()
        .join(iso_path.as_ref().file_name().unwrap());
    cso_path.set_extension(CSO_EXTENSION);

    progress_bar.println(format!(
        "Creating \"{}\"",
        cso_path.file_name().unwrap().to_str().unwrap()
    ));

    let output = Command::new(MAXCSO)
        .arg(iso_path.as_ref())
        .arg("-o")
        .arg(&cso_path)
        .output()
        .await
        .expect("Failed to create CSO");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(cso_path)
}

pub async fn extract_cso<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    cso_path: &P,
    directory: &Q,
) -> SimpleResult<PathBuf> {
    progress_bar.set_message("Extracting CSO");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    progress_bar.println(format!(
        "Extracting \"{}\"",
        cso_path.as_ref().file_name().unwrap().to_str().unwrap()
    ));

    let mut iso_path = directory
        .as_ref()
        .join(cso_path.as_ref().file_name().unwrap());
    iso_path.set_extension(ISO_EXTENSION);

    let output = Command::new(MAXCSO)
        .arg("--decompress")
        .arg(cso_path.as_ref())
        .arg("-o")
        .arg(&iso_path)
        .output()
        .await
        .expect("Failed to extract CSO");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(iso_path)
}

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

async fn create_xso<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    iso_path: &P,
    directory: &Q,
    extension: &str,
    format: (&str, &str),
) -> SimpleResult<PathBuf> {
    progress_bar.set_message(format!("Creating {}", format.0));
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    let mut xso_path = directory
        .as_ref()
        .join(iso_path.as_ref().file_name().unwrap());
    xso_path.set_extension(extension);

    progress_bar.println(format!(
        "Creating \"{}\"",
        xso_path.file_name().unwrap().to_str().unwrap()
    ));

    let output = Command::new(MAXCSO)
        .arg("--block=2048")
        .arg(format!("--format={}", format.1))
        .arg(iso_path.as_ref())
        .arg("-o")
        .arg(&xso_path)
        .output()
        .await
        .expect(&format!("Failed to create {}", format.0));

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(xso_path)
}

async fn extract_xso<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    xso_path: &P,
    directory: &Q,
    format: &str,
) -> SimpleResult<PathBuf> {
    progress_bar.set_message(format!("Extracting {}", format));
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    progress_bar.println(format!(
        "Extracting \"{}\"",
        xso_path.as_ref().file_name().unwrap().to_str().unwrap()
    ));

    let mut iso_path = directory
        .as_ref()
        .join(xso_path.as_ref().file_name().unwrap());
    iso_path.set_extension(ISO_EXTENSION);

    let output = Command::new(MAXCSO)
        .arg("--decompress")
        .arg(xso_path.as_ref())
        .arg("-o")
        .arg(&iso_path)
        .output()
        .await
        .expect(&format!("Failed to extract {}", format));

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(iso_path)
}

pub async fn create_cso<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    iso_path: &P,
    directory: &Q,
) -> SimpleResult<PathBuf> {
    let cso_path = create_xso(
        progress_bar,
        iso_path,
        directory,
        CSO_EXTENSION,
        ("CSO", "cso1"),
    )
    .await?;
    Ok(cso_path)
}

pub async fn extract_cso<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    cso_path: &P,
    directory: &Q,
) -> SimpleResult<PathBuf> {
    let iso_path = extract_xso(progress_bar, cso_path, directory, "CSO").await?;
    Ok(iso_path)
}

pub async fn create_zso<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    iso_path: &P,
    directory: &Q,
) -> SimpleResult<PathBuf> {
    let zso_path = create_xso(
        progress_bar,
        iso_path,
        directory,
        ZSO_EXTENSION,
        ("ZSO", "zso"),
    )
    .await?;
    Ok(zso_path)
}

pub async fn extract_zso<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    zso_path: &P,
    directory: &Q,
) -> SimpleResult<PathBuf> {
    let iso_path = extract_xso(progress_bar, zso_path, directory, "ZSO").await?;
    Ok(iso_path)
}

use super::config::*;
use super::progress::*;
use super::SimpleResult;
use indicatif::ProgressBar;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

const NSZ: &str = "nsz";

lazy_static! {
    static ref VERSION_REGEX: Regex = Regex::new(r"\d+\.\d+\.\d+").unwrap();
}

pub async fn get_version() -> SimpleResult<String> {
    let output = try_with!(
        Command::new(NSZ).arg("-h").output().await,
        "Failed to spawn nsz"
    );

    // nsz doesn't advertise any version
    String::from_utf8(output.stderr).unwrap();
    let version = String::from("unknown");

    Ok(version)
}

pub async fn create_nsz<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    nsp_path: &P,
    directory: &Q,
) -> SimpleResult<PathBuf> {
    progress_bar.set_message("Creating NSZ");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    let mut nsz_path = directory
        .as_ref()
        .join(nsp_path.as_ref().file_name().unwrap());
    nsz_path.set_extension(NSZ_EXTENSION);

    progress_bar.println(format!(
        "Creating \"{}\"",
        nsz_path.file_name().unwrap().to_str().unwrap()
    ));

    let output = Command::new(NSZ)
        .arg("-C")
        .arg("-L")
        .arg("-K")
        .arg("-o")
        .arg(directory.as_ref())
        .arg(nsp_path.as_ref())
        .output()
        .await
        .expect("Failed to create NSZ");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(nsz_path)
}

pub async fn extract_nsz<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    nsz_path: &P,
    directory: &Q,
) -> SimpleResult<PathBuf> {
    progress_bar.set_message("Extracting NSZ");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    progress_bar.println(format!(
        "Extracting \"{}\"",
        nsz_path.as_ref().file_name().unwrap().to_str().unwrap()
    ));

    let mut nsp_path = directory
        .as_ref()
        .join(nsz_path.as_ref().file_name().unwrap());
    nsp_path.set_extension(NSP_EXTENSION);

    let output = Command::new(NSZ)
        .arg("-D")
        .arg("-R")
        .arg("-o")
        .arg(directory.as_ref())
        .arg(nsz_path.as_ref())
        .output()
        .await
        .expect("Failed to extract NSZ");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(nsp_path)
}

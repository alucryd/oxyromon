use super::progress::*;
use super::SimpleResult;
use async_std::path::{Path, PathBuf};
use indicatif::ProgressBar;
use std::process::Command;

pub static CSO_EXTENSION: &str = "cso";
pub static ISO_EXTENSION: &str = "iso";

pub fn create_cso<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    iso_path: &P,
    directory: &Q,
) -> SimpleResult<PathBuf> {
    progress_bar.set_message("Creating CSO");
    progress_bar.set_style(get_none_progress_style());

    let mut cso_path = directory
        .as_ref()
        .join(iso_path.as_ref().file_name().unwrap());
    cso_path.set_extension(CSO_EXTENSION);

    let output = Command::new("maxcso")
        .arg(iso_path.as_ref())
        .arg("-o")
        .arg(&cso_path)
        .output()
        .expect("Failed to create CSO");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    Ok(cso_path)
}

pub fn extract_cso<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    cso_path: &P,
    directory: &Q,
) -> SimpleResult<PathBuf> {
    progress_bar.set_message("Extracting CSO");
    progress_bar.set_style(get_none_progress_style());

    let mut iso_path = directory
        .as_ref()
        .join(cso_path.as_ref().file_name().unwrap());
    iso_path.set_extension(ISO_EXTENSION);

    let output = Command::new("maxcso")
        .arg("--decompress")
        .arg(cso_path.as_ref())
        .arg("-o")
        .arg(&iso_path)
        .output()
        .expect("Failed to extract CSO");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    Ok(iso_path)
}

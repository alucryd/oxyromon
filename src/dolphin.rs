use super::config::*;
use super::progress::*;
use super::SimpleResult;
use async_std::path::{Path, PathBuf};
use indicatif::ProgressBar;
use std::process::Command;

pub fn create_rvz<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    iso_path: &P,
    directory: &Q,
) -> SimpleResult<PathBuf> {
    progress_bar.set_message("Creating RVZ");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(100);

    let mut rvz_path = directory
        .as_ref()
        .join(iso_path.as_ref().file_name().unwrap());
    rvz_path.set_extension(RVZ_EXTENSION);

    progress_bar.println(format!("Creating {:?}", rvz_path.file_name().unwrap()));

    let output = Command::new("dolphin-tool")
        .arg("convert")
        .arg("-f")
        .arg("rvz")
        .arg("-c")
        .arg("zstd")
        .arg("-l")
        .arg("5")
        .arg("-b")
        .arg("131072")
        .arg("-i")
        .arg(iso_path.as_ref())
        .arg("-o")
        .arg(&rvz_path)
        .output()
        .expect("Failed to create RVZ");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(rvz_path)
}

pub fn extract_rvz<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    rvz_path: &P,
    directory: &Q,
) -> SimpleResult<PathBuf> {
    progress_bar.set_message("Extracting RVZ");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(100);

    progress_bar.println(format!(
        "Extracting {:?}",
        rvz_path.as_ref().file_name().unwrap()
    ));

    let mut iso_path = directory
        .as_ref()
        .join(rvz_path.as_ref().file_name().unwrap());
    iso_path.set_extension(ISO_EXTENSION);

    let output = Command::new("dolphin-tool")
        .arg("convert")
        .arg("-f")
        .arg("iso")
        .arg("-i")
        .arg(rvz_path.as_ref())
        .arg("-o")
        .arg(&iso_path)
        .output()
        .expect("Failed to extract RVZ");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(iso_path)
}

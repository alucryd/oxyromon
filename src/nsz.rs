use super::config::*;
use super::progress::*;
use super::SimpleResult;
use indicatif::ProgressBar;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub fn create_nsz<P: AsRef<Path>, Q: AsRef<Path>>(
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

    let output = Command::new("nsz")
        .arg("-C")
        .arg("-L")
        .arg("-K")
        .arg("-o")
        .arg(directory.as_ref())
        .arg(nsp_path.as_ref())
        .output()
        .expect("Failed to create NSZ");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(nsz_path)
}

pub fn extract_nsz<P: AsRef<Path>, Q: AsRef<Path>>(
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

    let output = Command::new("nsz")
        .arg("-D")
        .arg("-R")
        .arg("-o")
        .arg(directory.as_ref())
        .arg(nsz_path.as_ref())
        .output()
        .expect("Failed to extract NSZ");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(nsp_path)
}

use super::progress::*;
use super::util::*;
use super::SimpleResult;
use async_std::io;
use async_std::path::{Path, PathBuf};
use async_std::prelude::*;
use indicatif::ProgressBar;
use std::process::Command;

pub static CHD_EXTENSION: &str = "chd";
pub static CUE_EXTENSION: &str = "cue";
pub static BIN_EXTENSION: &str = "bin";

pub fn create_chd(cue_path: &PathBuf, progress_bar: &ProgressBar) -> SimpleResult<PathBuf> {
    progress_bar.set_message("Creating CHD");
    progress_bar.set_style(get_none_progress_style());

    let mut chd_path = cue_path.clone();
    chd_path.set_extension(CHD_EXTENSION);

    let output = Command::new("chdman")
        .arg("createcd")
        .arg("-i")
        .arg(cue_path)
        .arg("-o")
        .arg(&chd_path)
        .output()
        .expect("Failed to create CHD");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    Ok(chd_path)
}

pub async fn extract_chd(
    chd_path: &PathBuf,
    directory: &Path,
    bin_names_sizes: &[(&str, u64)],
    progress_bar: &ProgressBar,
) -> SimpleResult<Vec<PathBuf>> {
    progress_bar.set_message("Extracting CHD");
    progress_bar.set_style(get_none_progress_style());

    let mut bin_path = directory.join(chd_path.file_name().unwrap());
    bin_path.set_extension(BIN_EXTENSION);

    let mut cue_name = bin_path.file_name().unwrap().to_os_string();
    cue_name.push(".");
    cue_name.push(CUE_EXTENSION);
    let cue_path = directory.join(cue_name);

    let output = Command::new("chdman")
        .arg("extractcd")
        .arg("-i")
        .arg(chd_path)
        .arg("-o")
        .arg(&cue_path)
        .arg("-ob")
        .arg(&bin_path)
        .output()
        .expect("Failed to spawn chdman process");

    remove_file(&cue_path).await?;

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str());
    }

    if bin_names_sizes.len() == 1 {
        let new_bin_path = directory.join(bin_names_sizes.get(0).unwrap().0);
        if bin_path != new_bin_path {
            rename_file(&bin_path, &new_bin_path).await?;
        }
        return Ok(vec![new_bin_path]);
    }

    let mut bin_paths: Vec<PathBuf> = Vec::new();
    let bin_file = open_file(&bin_path).await?;

    for (bin_name, size) in bin_names_sizes {
        progress_bar.set_length(*size);

        let split_bin_path = directory.join(bin_name);
        let mut split_bin_file = create_file(&split_bin_path).await?;

        let mut handle = (&bin_file).take(*size);

        io::copy(&mut handle, &mut split_bin_file)
            .await
            .expect("Failed to copy data");

        bin_paths.push(split_bin_path);
    }

    remove_file(&bin_path).await?;

    Ok(bin_paths)
}

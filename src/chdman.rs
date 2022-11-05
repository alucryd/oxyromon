use super::config::*;
use super::progress::*;
use super::util::*;
use super::SimpleResult;
use async_std::io;
use async_std::path::{Path, PathBuf};
use async_std::prelude::*;
use indicatif::ProgressBar;
use std::process::Command;
use std::time::Duration;

pub fn create_chd<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    romfile_path: &P,
    directory: &Q,
) -> SimpleResult<PathBuf> {
    progress_bar.set_message("Creating CHD");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    let mut chd_path = directory
        .as_ref()
        .join(romfile_path.as_ref().file_name().unwrap());
    chd_path.set_extension(CHD_EXTENSION);

    progress_bar.println(format!("Creating {:?}", chd_path.file_name().unwrap()));

    let output = Command::new("chdman")
        .arg("createcd")
        .arg("-i")
        .arg(romfile_path.as_ref())
        .arg("-o")
        .arg(&chd_path)
        .output()
        .expect("Failed to create CHD");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(chd_path)
}

pub async fn extract_chd_to_multiple_tracks<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    chd_path: &P,
    directory: &Q,
    bin_names_sizes: &[(&str, u64)],
    quiet: bool,
) -> SimpleResult<Vec<PathBuf>> {
    progress_bar.set_message("Extracting CHD");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    progress_bar.println(format!(
        "Extracting {:?}",
        chd_path.as_ref().file_name().unwrap()
    ));

    let cue_path = directory.as_ref().join(format!(
        "{}.{}",
        chd_path.as_ref().file_name().unwrap().to_str().unwrap(),
        CUE_EXTENSION
    ));
    let mut bin_path = directory
        .as_ref()
        .join(chd_path.as_ref().file_name().unwrap());
    bin_path.set_extension(BIN_EXTENSION);

    let output = Command::new("chdman")
        .arg("extractcd")
        .arg("-i")
        .arg(chd_path.as_ref())
        .arg("-o")
        .arg(&cue_path)
        .arg("-ob")
        .arg(&bin_path)
        .output()
        .expect("Failed to spawn chdman process");

    remove_file(progress_bar, &cue_path, true).await?;

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str());
    }

    if bin_names_sizes.len() == 1 {
        let new_bin_path = directory.as_ref().join(bin_names_sizes.first().unwrap().0);
        if bin_path != new_bin_path {
            rename_file(progress_bar, &bin_path, &new_bin_path, quiet).await?;
        }
        return Ok(vec![new_bin_path]);
    }

    let mut bin_paths: Vec<PathBuf> = Vec::new();
    let bin_file = open_file(&bin_path).await?;

    for (bin_name, size) in bin_names_sizes {
        progress_bar.set_length(*size);

        let split_bin_path = directory.as_ref().join(bin_name);
        let mut split_bin_file = create_file(progress_bar, &split_bin_path, quiet).await?;

        let mut handle = (&bin_file).take(*size);

        io::copy(&mut handle, &mut split_bin_file)
            .await
            .expect("Failed to copy data");

        bin_paths.push(split_bin_path);
    }

    remove_file(progress_bar, &bin_path, quiet).await?;

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(bin_paths)
}

pub async fn extract_chd_to_single_track<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    chd_path: &P,
    directory: &Q,
) -> SimpleResult<PathBuf> {
    progress_bar.set_message("Extracting CHD");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    let cue_path = directory.as_ref().join(format!(
        "{}.{}",
        chd_path.as_ref().file_name().unwrap().to_str().unwrap(),
        CUE_EXTENSION
    ));
    let mut bin_path = directory
        .as_ref()
        .join(chd_path.as_ref().file_name().unwrap());
    bin_path.set_extension(BIN_EXTENSION);

    let output = Command::new("chdman")
        .arg("extractcd")
        .arg("-i")
        .arg(chd_path.as_ref())
        .arg("-o")
        .arg(&cue_path)
        .arg("-ob")
        .arg(&bin_path)
        .output()
        .expect("Failed to spawn chdman process");

    remove_file(progress_bar, &cue_path, true).await?;

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str());
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(bin_path)
}

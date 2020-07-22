use super::progress::*;
use super::SimpleResult;
use indicatif::ProgressBar;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

pub static SEVENZIP_EXTENSION: &str = "7z";
pub static ZIP_EXTENSION: &str = "zip";
pub static ARCHIVE_EXTENSIONS: [&str; 2] = [SEVENZIP_EXTENSION, ZIP_EXTENSION];

pub enum ArchiveType {
    SEVENZIP,
    ZIP,
}

pub struct ArchiveInfo {
    pub path: String,
    pub size: u64,
    pub crc: String,
}

pub fn parse_archive(
    archive_path: &PathBuf,
    progress_bar: &ProgressBar,
) -> SimpleResult<Vec<ArchiveInfo>> {
    progress_bar.set_message("Parsing archive");
    progress_bar.set_style(get_none_progress_style());

    let output = Command::new("7z")
        .arg("l")
        .arg("-slt")
        .arg(&archive_path)
        .output()
        .expect("Failed to parse archive");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str());
    }

    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout
        .lines()
        .filter(|&line| {
            line.starts_with("Path =") || line.starts_with("Size =") || line.starts_with("CRC =")
        })
        .skip(1) // the first line is the archive itself
        .map(|line| line.split("=").last().unwrap().trim()) // keep only the rhs
        .collect();

    // each chunk will have the path, size and crc respectively
    let mut sevenzip_infos: Vec<ArchiveInfo> = Vec::new();
    for info in lines.chunks(3) {
        let sevenzip_info = ArchiveInfo {
            path: String::from(info.get(0).unwrap().to_owned()),
            size: FromStr::from_str(info.get(1).unwrap()).unwrap(),
            crc: String::from(info.get(2).unwrap().to_owned()),
        };
        sevenzip_infos.push(sevenzip_info);
    }

    Ok(sevenzip_infos)
}

pub fn rename_file_in_archive(
    archive_path: &PathBuf,
    file_name: &str,
    new_file_name: &str,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    progress_bar.set_message("Renaming file in archive");
    progress_bar.set_style(get_none_progress_style());

    let output = Command::new("7z")
        .arg("rn")
        .arg(archive_path)
        .arg(file_name)
        .arg(new_file_name)
        .output()
        .expect("Failed to rename file in archive");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str());
    }

    Ok(())
}

pub fn extract_files_from_archive(
    archive_path: &PathBuf,
    file_names: &Vec<&str>,
    directory: &Path,
    progress_bar: &ProgressBar,
) -> SimpleResult<Vec<PathBuf>> {
    progress_bar.set_message("Extracting archive");
    progress_bar.set_style(get_none_progress_style());

    let output = Command::new("7z")
        .arg("x")
        .arg(archive_path)
        .args(file_names)
        .current_dir(directory)
        .output()
        .expect("Failed to extract archive");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    Ok(file_names
        .iter()
        .map(|file_name| directory.join(file_name))
        .collect())
}

pub fn add_files_to_archive(
    archive_path: &PathBuf,
    file_names: &Vec<&str>,
    directory: &Path,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    progress_bar.set_message("Extracting archive");
    progress_bar.set_style(get_none_progress_style());

    let output = Command::new("7z")
        .arg("a")
        .arg(archive_path)
        .args(file_names)
        .arg("-mx=9")
        .current_dir(directory)
        .output()
        .expect("Failed to create archive");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    Ok(())
}

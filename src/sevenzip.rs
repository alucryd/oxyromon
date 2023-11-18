use super::progress::*;
use super::SimpleResult;
use indicatif::ProgressBar;
use std::fs::{File, OpenOptions};
use std::iter::zip;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;
use tokio::process::Command;
use zip::{ZipArchive, ZipWriter};

pub const SEVENZIP_COMPRESSION_LEVEL_RANGE: [usize; 2] = [1, 9];
pub const ZIP_COMPRESSION_LEVEL_RANGE: [usize; 2] = [1, 9];

#[derive(PartialEq, Eq)]
pub enum ArchiveType {
    Sevenzip,
    Zip,
}

pub struct ArchiveInfo {
    pub path: String,
    pub size: u64,
    pub crc: String,
}

pub async fn parse_archive<P: AsRef<Path>>(
    progress_bar: &ProgressBar,
    archive_path: &P,
) -> SimpleResult<Vec<ArchiveInfo>> {
    progress_bar.set_message("Parsing archive");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    let output = Command::new("7z")
        .arg("l")
        .arg("-slt")
        .arg(archive_path.as_ref())
        .output()
        .await
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
        .map(|line| line.split('=').last().unwrap().trim()) // keep only the rhs
        .collect();

    // each chunk will have the path, size and crc respectively
    let mut sevenzip_infos: Vec<ArchiveInfo> = Vec::new();
    for info in lines.chunks(3) {
        if let [path, size, crc] = info {
            let sevenzip_info = ArchiveInfo {
                path: path.to_string(),
                size: u64::from_str(size).unwrap(),
                crc: crc.to_lowercase(),
            };
            sevenzip_infos.push(sevenzip_info);
        }
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(sevenzip_infos)
}

pub async fn rename_file_in_archive<P: AsRef<Path>>(
    progress_bar: &ProgressBar,
    archive_path: &P,
    file_name: &str,
    new_file_name: &str,
) -> SimpleResult<()> {
    progress_bar.set_message("Renaming file in archive");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));
    progress_bar.println(format!(
        "Renaming \"{}\" to \"{}\"",
        file_name, new_file_name
    ));

    let output = Command::new("7z")
        .arg("rn")
        .arg(archive_path.as_ref())
        .arg(file_name)
        .arg(new_file_name)
        .output()
        .await
        .expect("Failed to rename file in archive");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str());
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(())
}

pub async fn extract_files_from_archive<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    archive_path: &P,
    file_names: &[&str],
    directory: &Q,
) -> SimpleResult<Vec<PathBuf>> {
    progress_bar.set_message("Extracting files");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    for &file_name in file_names {
        progress_bar.println(format!("Extracting \"{}\"", file_name));
    }

    let output = Command::new("7z")
        .arg("x")
        .arg(archive_path.as_ref())
        .args(file_names)
        .current_dir(directory.as_ref())
        .output()
        .await
        .expect("Failed to extract archive");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(file_names
        .iter()
        .map(|file_name| directory.as_ref().join(file_name))
        .collect())
}

pub async fn add_files_to_archive<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    archive_path: &P,
    file_names: &[&str],
    directory: &Q,
    compression_level: usize,
    solid: bool,
) -> SimpleResult<()> {
    progress_bar.set_message("Compressing files");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    for &file_name in file_names {
        progress_bar.println(format!("Compressing \"{}\"", file_name));
    }

    let mut args = vec![format!("-mx={}", compression_level)];
    if solid {
        args.push(String::from("-ms=on"))
    }
    let output = Command::new("7z")
        .arg("a")
        .arg(archive_path.as_ref())
        .args(file_names)
        .args(args)
        .current_dir(directory.as_ref())
        .output()
        .await
        .expect("Failed to add files to archive");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(())
}

pub async fn remove_files_from_archive<P: AsRef<Path>>(
    progress_bar: &ProgressBar,
    archive_path: &P,
    file_names: &[&str],
) -> SimpleResult<()> {
    progress_bar.set_message("Deleting files");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    for &file_name in file_names {
        progress_bar.println(format!("Deleting \"{}\"", file_name));
    }

    let output = Command::new("7z")
        .arg("d")
        .arg(archive_path.as_ref())
        .args(file_names)
        .output()
        .await
        .expect("Failed to remove files from archive");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str())
    }

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(())
}

pub async fn copy_files_between_archives<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    source_path: &P,
    destination_path: &Q,
    source_names: &[&str],
    destination_names: &[&str],
) -> SimpleResult<()> {
    progress_bar.set_message("Copying files between archives");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    let source_file = File::open(source_path.as_ref()).expect("Failed to read archive");
    let mut source_archive = ZipArchive::new(source_file).expect("Failed to open archive");

    let destination_file: File;
    let mut destination_archive: ZipWriter<File>;
    if destination_path.as_ref().is_file() {
        destination_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(destination_path.as_ref())
            .expect("Failed to open archive");
        destination_archive =
            ZipWriter::new_append(destination_file).expect("Failed to open archive");
    } else {
        destination_file =
            File::create(destination_path.as_ref()).expect("Failed to create archive");
        destination_archive = ZipWriter::new(destination_file);
    };

    for (&source_name, &destination_name) in zip(source_names, destination_names) {
        if source_name == destination_name {
            progress_bar.println(format!("Copying \"{}\"", source_name));
            destination_archive
                .raw_copy_file(source_archive.by_name(source_name).unwrap())
                .expect("Failed to copy file")
        } else {
            progress_bar.println(format!(
                "Copying \"{}\" to \"{}\"",
                source_name, destination_name
            ));
            destination_archive
                .raw_copy_file_rename(
                    source_archive.by_name(source_name).unwrap(),
                    destination_name,
                )
                .expect("Failed to copy file")
        }
    }

    Ok(())
}

use super::SimpleResult;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

pub enum ArchiveType {
    SEVENZIP,
    ZIP,
}

pub struct ArchiveInfo {
    pub path: String,
    pub size: u64,
    pub crc: String,
}

pub fn parse_archive(archive_path: &PathBuf) -> SimpleResult<Vec<ArchiveInfo>> {
    println!("Scanning {:?}", archive_path.file_name().unwrap());
    let output = try_with!(
        Command::new("7z")
            .arg("l")
            .arg("-slt")
            .arg(&archive_path)
            .output(),
        "Failed to parse archive"
    );
    if !output.status.success() {
        let stderr = try_with!(String::from_utf8(output.stderr), "Failed to get stderr");
        println!("{}", stderr);
        bail!(stderr.as_str());
    }

    let stdout = try_with!(String::from_utf8(output.stdout), "Failed to get stdout");
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

pub fn move_file_in_archive(
    archive_path: &PathBuf,
    file_name: &str,
    new_file_name: &str,
) -> SimpleResult<()> {
    println!("Renaming \"{}\" to \"{}\"", file_name, new_file_name);
    let output = try_with!(
        Command::new("7z")
            .arg("rn")
            .arg(archive_path)
            .arg(file_name)
            .arg(new_file_name)
            .output(),
        "Failed to rename file in archive"
    );
    if !output.status.success() {
        let stderr = try_with!(String::from_utf8(output.stderr), "Failed to get stderr");
        println!("{}", stderr);
        bail!(stderr.as_str());
    }
    Ok(())
}

pub fn extract_files_from_archive(
    archive_path: &PathBuf,
    file_names: &Vec<&str>,
    directory: &Path,
) -> SimpleResult<()> {
    println!(
        "Extracting {:?} from {:?}",
        file_names,
        archive_path.file_name().unwrap()
    );
    let output = try_with!(
        Command::new("7z")
            .arg("x")
            .arg(archive_path)
            .args(file_names)
            .current_dir(directory)
            .output(),
        "Failed to extract files from archive"
    );
    if !output.status.success() {
        let stderr = try_with!(String::from_utf8(output.stderr), "Failed to get stderr");
        println!("{}", stderr);
        bail!(stderr.as_str())
    }
    Ok(())
}

pub fn add_files_to_archive(
    archive_path: &PathBuf,
    file_names: &Vec<&str>,
    directory: &Path,
) -> SimpleResult<()> {
    println!(
        "Compressing {:?} to {:?}",
        file_names,
        archive_path.file_name().unwrap()
    );
    let output = try_with!(
        Command::new("7z")
            .arg("a")
            .arg(archive_path)
            .args(file_names)
            .arg("-mx=9")
            .current_dir(directory)
            .output(),
        "Failed to add files to archive"
    );
    if !output.status.success() {
        let stderr = try_with!(String::from_utf8(output.stderr), "Failed to get stderr");
        println!("{}", stderr);
        bail!(stderr.as_str())
    }
    Ok(())
}

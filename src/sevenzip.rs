use super::common::*;
use super::config::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use super::SimpleResult;
use cfg_if::cfg_if;
use indicatif::ProgressBar;
use regex::Regex;
use sqlx::SqliteConnection;
use std::fs::{File, OpenOptions};
use std::iter::zip;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;
use tokio::process::Command;
use zip::{ZipArchive, ZipWriter};

cfg_if! {
    if #[cfg(macos)] {
        const SEVENZIP: &str = "7zz";
    } else {
        const SEVENZIP: &str = "7z";
    }
}

pub const SEVENZIP_COMPRESSION_LEVEL_RANGE: [usize; 2] = [1, 9];
pub const ZIP_COMPRESSION_LEVEL_RANGE: [usize; 2] = [1, 9];

lazy_static! {
    static ref VERSION_REGEX: Regex = Regex::new(r"\d+\.\d+").unwrap();
}

#[derive(PartialEq, Eq)]
pub enum ArchiveType {
    Sevenzip,
    Zip,
}

#[derive(Clone)]
pub struct ArchiveRomfile {
    pub path: PathBuf,
    pub file_path: String,
}

pub trait ArchiveFile {
    async fn rename_file(
        &self,
        progress_bar: &ProgressBar,
        new_file_path: &str,
    ) -> SimpleResult<ArchiveRomfile>;
    async fn delete_file(self, progress_bar: &ProgressBar) -> SimpleResult<()>;
}

impl CommonFile for ArchiveRomfile {
    async fn rename<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        new_path: &P,
        quiet: bool,
    ) -> SimpleResult<OriginalRomfile> {
        rename_file(progress_bar, &self.path, new_path, quiet).await?;
        Ok(OriginalRomfile {
            path: new_path.as_ref().to_path_buf(),
        })
    }

    async fn delete(self, progress_bar: &ProgressBar, quiet: bool) -> SimpleResult<()> {
        remove_file(progress_bar, &self.path, quiet).await?;
        Ok(())
    }
}

impl ArchiveFile for ArchiveRomfile {
    async fn rename_file(
        &self,
        progress_bar: &ProgressBar,
        new_file_path: &str,
    ) -> SimpleResult<ArchiveRomfile> {
        progress_bar.set_message("Renaming file in archive");
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));
        progress_bar.println(format!(
            "Renaming \"{}\" to \"{}\"",
            &self.file_path, new_file_path
        ));

        let output = Command::new(SEVENZIP)
            .arg("rn")
            .arg(&self.path)
            .arg(&self.file_path)
            .arg(new_file_path)
            .output()
            .await
            .expect("Failed to rename file in archive");

        if !output.status.success() {
            bail!(String::from_utf8(output.stderr).unwrap().as_str());
        }

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        Ok(ArchiveRomfile {
            path: self.path.clone(),
            file_path: new_file_path.to_string(),
        })
    }

    async fn delete_file(self, progress_bar: &ProgressBar) -> SimpleResult<()> {
        progress_bar.set_message("Deleting files");
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        progress_bar.println(format!("Deleting \"{}\"", &self.file_path));

        let output = Command::new(SEVENZIP)
            .arg("d")
            .arg(&self.path)
            .arg(&self.file_path)
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
}

impl ToOriginal for ArchiveRomfile {
    async fn to_original<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        directory: &P,
    ) -> SimpleResult<OriginalRomfile> {
        progress_bar.set_message("Extracting file");
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        progress_bar.println(format!("Extracting \"{}\"", &self.file_path));

        let output = Command::new(SEVENZIP)
            .arg("x")
            .arg(&self.path)
            .arg(&self.file_path)
            .current_dir(directory.as_ref())
            .output()
            .await
            .expect("Failed to extract archive");

        if !output.status.success() {
            bail!(String::from_utf8(output.stderr).unwrap().as_str())
        }

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        Ok(OriginalRomfile {
            path: directory.as_ref().join(&self.file_path),
        })
    }
}

impl Size for ArchiveRomfile {
    async fn get_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
    ) -> SimpleResult<u64> {
        match header.is_some() {
            true => {
                let tmp_directory = create_tmp_directory(connection).await?;
                let original_file = self
                    .clone()
                    .to_original(progress_bar, &tmp_directory)
                    .await?;
                let size = original_file
                    .get_size(connection, progress_bar, header)
                    .await?;
                remove_file(progress_bar, &original_file.path, true).await?;
                Ok(size)
            }
            false => {
                let output = Command::new(SEVENZIP)
                    .arg("l")
                    .arg("-slt")
                    .arg(&self.path)
                    .arg(&self.file_path)
                    .output()
                    .await
                    .expect("Failed to parse archive");

                if !output.status.success() {
                    bail!(String::from_utf8(output.stderr).unwrap().as_str());
                }

                let stdout = String::from_utf8(output.stdout).unwrap();
                let size: &str = stdout
                    .lines()
                    .find(|&line| line.starts_with("Size ="))
                    .map(|line| line.split('=').last().unwrap().trim()) // keep only the rhs
                    .unwrap();
                Ok(u64::from_str(size).unwrap())
            }
        }
    }
}

impl Checksum for ArchiveRomfile {
    async fn get_hash(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> simple_error::SimpleResult<String> {
        match header.is_some() || hash_algorithm != &HashAlgorithm::Crc {
            true => {
                let tmp_directory = create_tmp_directory(connection).await?;
                let original_file = self
                    .clone()
                    .to_original(progress_bar, &tmp_directory)
                    .await?;
                let hash = original_file
                    .get_hash(
                        connection,
                        progress_bar,
                        header,
                        position,
                        total,
                        hash_algorithm,
                    )
                    .await?;
                remove_file(progress_bar, &original_file.path, true).await?;
                Ok(hash)
            }
            false => {
                let output = Command::new(SEVENZIP)
                    .arg("l")
                    .arg("-slt")
                    .arg(&self.path)
                    .arg(&self.file_path)
                    .output()
                    .await
                    .expect("Failed to parse archive");

                if !output.status.success() {
                    bail!(String::from_utf8(output.stderr).unwrap().as_str());
                }

                let stdout = String::from_utf8(output.stdout).unwrap();
                let hash: &str = stdout
                    .lines()
                    .find(|&line| line.starts_with("CRC ="))
                    .map(|line| line.split('=').last().unwrap().trim()) // keep only the rhs
                    .unwrap();
                Ok(hash.to_string())
            }
        }
    }

    async fn check(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        rom: &Rom,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> SimpleResult<()> {
        let size = self.get_size(connection, progress_bar, header).await?;
        if size != rom.size as u64 {
            bail!("Size mismatch");
        };

        let hash = self
            .get_hash(
                connection,
                progress_bar,
                header,
                position,
                total,
                hash_algorithm,
            )
            .await?;
        match hash_algorithm {
            HashAlgorithm::Crc => {
                if &hash != rom.crc.as_ref().unwrap() {
                    bail!("Checksum mismatch");
                }
            }
            HashAlgorithm::Md5 => {
                if &hash != rom.md5.as_ref().unwrap() {
                    bail!("Checksum mismatch");
                }
            }
            HashAlgorithm::Sha1 => {
                if &hash != rom.sha1.as_ref().unwrap() {
                    bail!("Checksum mismatch");
                }
            }
        }

        Ok(())
    }
}

pub trait AsArchive {
    fn as_archive(&self, rom: &Rom) -> ArchiveRomfile;
}

impl AsArchive for Romfile {
    fn as_archive(&self, rom: &Rom) -> ArchiveRomfile {
        ArchiveRomfile {
            path: PathBuf::from(self.path),
            file_path: rom.name.clone(),
        }
    }
}

pub trait ToArchive {
    async fn to_archive<P: AsRef<Path>, Q: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        archive_path: &P,
        directory: &Q,
        compression_level: usize,
        solid: bool,
    ) -> SimpleResult<ArchiveRomfile>;
}

impl ToArchive for OriginalRomfile {
    async fn to_archive<P: AsRef<Path>, Q: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        archive_path: &P,
        directory: &Q,
        compression_level: usize,
        solid: bool,
    ) -> SimpleResult<ArchiveRomfile> {
        progress_bar.set_message("Compressing file");
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        progress_bar.println(format!(
            "Compressing \"{}\"",
            &self.path.as_os_str().to_str().unwrap()
        ));

        let mut args = vec![format!("-mx={}", compression_level)];
        if solid {
            args.push(String::from("-ms=on"))
        }
        let output = Command::new(SEVENZIP)
            .arg("a")
            .arg(archive_path.as_ref())
            .arg(&self.path)
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

        Ok(ArchiveRomfile {
            path: archive_path.as_ref().to_path_buf(),
            file_path: self.path.as_os_str().to_str().unwrap().to_string(),
        })
    }
}

pub async fn parse<P: AsRef<Path>>(
    progress_bar: &ProgressBar,
    path: &P,
) -> SimpleResult<Vec<ArchiveRomfile>> {
    progress_bar.set_message("Parsing archive");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    let output = Command::new(SEVENZIP)
        .arg("l")
        .arg("-slt")
        .arg(path.as_ref())
        .output()
        .await
        .expect("Failed to parse archive");

    if !output.status.success() {
        bail!(String::from_utf8(output.stderr).unwrap().as_str());
    }

    let stdout = String::from_utf8(output.stdout).unwrap();
    let file_paths: Vec<&str> = stdout
        .lines()
        .filter(|&line| line.starts_with("Path ="))
        .skip(1) // the first line is the archive itself
        .map(|line| line.split('=').last().unwrap().trim()) // keep only the rhs
        .collect();

    let archived_romfiles: Vec<ArchiveRomfile> = file_paths
        .into_iter()
        .map(|file_path| ArchiveRomfile {
            path: path.as_ref().to_path_buf(),
            file_path: file_path.to_string(),
        })
        .collect();

    progress_bar.set_message("");
    progress_bar.disable_steady_tick();

    Ok(archived_romfiles)
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

pub async fn get_version() -> SimpleResult<String> {
    let output = try_with!(Command::new(SEVENZIP).output().await, "Failed to spawn 7z");

    let stdout = String::from_utf8(output.stdout).unwrap();
    let version = stdout
        .lines()
        .nth(1)
        .map(|line| VERSION_REGEX.find(line))
        .flatten()
        .map(|version| version.as_str().to_string())
        .unwrap_or(String::from("unknown"));

    Ok(version)
}

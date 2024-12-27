use super::common::*;
use super::config::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use super::SimpleResult;
use indicatif::ProgressBar;
use regex::Regex;
use sqlx::SqliteConnection;
use std::fs::{File, OpenOptions};
use std::iter::zip;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;
use strum::{Display, EnumString};
use tokio::process::Command;
use zip::{ZipArchive, ZipWriter};

pub const SEVENZIP_EXECUTABLES: &[&str] = &["7zz", "7z"];
pub const SEVENZIP_COMPRESSION_LEVEL_RANGE: [usize; 2] = [1, 9];
pub const ZIP_COMPRESSION_LEVEL_RANGE: [usize; 2] = [1, 9];

lazy_static! {
    static ref VERSION_REGEX: Regex = Regex::new(r"\d+\.\d+").unwrap();
}

#[derive(Clone, Copy, Display, EnumString, PartialEq, Eq)]
#[strum(serialize_all = "lowercase")]
pub enum ArchiveType {
    #[strum(serialize = "7z")]
    Sevenzip,
    Zip,
}

pub struct ArchiveRomfile {
    pub romfile: CommonRomfile,
    pub path: String,
    pub archive_type: ArchiveType,
}

pub trait ArchiveFile {
    async fn rename_file(
        &self,
        progress_bar: &ProgressBar,
        new_path: &str,
    ) -> SimpleResult<ArchiveRomfile>;
    async fn delete_file(&self, progress_bar: &ProgressBar) -> SimpleResult<()>;
}

impl ArchiveFile for ArchiveRomfile {
    async fn rename_file(
        &self,
        progress_bar: &ProgressBar,
        new_path: &str,
    ) -> SimpleResult<ArchiveRomfile> {
        progress_bar.set_message("Renaming file in archive");
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));
        progress_bar.println(format!("Renaming \"{}\" to \"{}\"", &self.path, new_path));

        let output = Command::new(get_executable_path(SEVENZIP_EXECUTABLES)?)
            .arg("rn")
            .arg(&self.romfile.path)
            .arg(self.path.replace("-", "?").replace("@", "?"))
            .arg(new_path)
            .output()
            .await
            .expect("Failed to rename file in archive");

        if !output.status.success() {
            bail!(String::from_utf8(output.stderr).unwrap().as_str());
        }

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        Ok(ArchiveRomfile {
            romfile: self.romfile.clone(),
            path: new_path.to_string(),
            archive_type: self.archive_type,
        })
    }

    async fn delete_file(&self, progress_bar: &ProgressBar) -> SimpleResult<()> {
        progress_bar.set_message("Deleting files");
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        progress_bar.println(format!("Deleting \"{}\"", &self.path));

        let output = Command::new(get_executable_path(SEVENZIP_EXECUTABLES)?)
            .arg("d")
            .arg(&self.romfile.path)
            .arg(self.path.replace("-", "?").replace("@", "?"))
            .output()
            .await
            .expect("Failed to remove files from archive");

        if !output.status.success() {
            bail!(String::from_utf8(output.stderr).unwrap().as_str())
        }

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        if self.romfile.as_archives(progress_bar).await?.is_empty() {
            self.romfile.delete(progress_bar, false).await?;
        }

        Ok(())
    }
}

impl Size for ArchiveRomfile {
    async fn get_size(&self) -> SimpleResult<u64> {
        let output = Command::new(get_executable_path(SEVENZIP_EXECUTABLES)?)
            .arg("l")
            .arg("-slt")
            .arg(&self.romfile.path)
            .arg(self.path.replace("-", "?").replace("@", "?"))
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

impl HashAndSize for ArchiveRomfile {
    async fn get_hash_and_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> SimpleResult<(String, u64)> {
        if hash_algorithm == &HashAlgorithm::Crc {
            let output = Command::new(get_executable_path(SEVENZIP_EXECUTABLES)?)
                .arg("l")
                .arg("-slt")
                .arg(&self.romfile.path)
                .arg(self.path.replace("-", "?").replace("@", "?"))
                .output()
                .await
                .expect("Failed to parse archive");

            if !output.status.success() {
                bail!(String::from_utf8(output.stderr).unwrap().as_str());
            }

            let stdout = String::from_utf8(output.stdout).unwrap();
            let hash = stdout
                .lines()
                .find(|&line| line.starts_with("CRC ="))
                .map(|line| line.split('=').last().unwrap().trim()) // keep only the rhs
                .unwrap()
                .to_string()
                .to_lowercase();
            let size = self.get_size().await?;
            Ok((hash, size))
        } else {
            let tmp_directory = create_tmp_directory(connection).await?;
            let (hash, size) = self
                .to_common(progress_bar, &tmp_directory)
                .await?
                .get_hash_and_size(connection, progress_bar, position, total, hash_algorithm)
                .await?;
            Ok((hash, size))
        }
    }
}

impl Check for ArchiveRomfile {
    async fn check(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        roms: &[&Rom],
    ) -> SimpleResult<()> {
        progress_bar.println(format!("Checking \"{}\" ({})", &self.romfile, &self.path));
        let rom = roms[0];
        let hash_algorithm: HashAlgorithm;
        if rom.crc.is_some() {
            hash_algorithm = HashAlgorithm::Crc;
        } else if rom.md5.is_some() {
            hash_algorithm = HashAlgorithm::Md5;
        } else if rom.sha1.is_some() {
            hash_algorithm = HashAlgorithm::Sha1;
        } else {
            bail!("Not possible")
        }
        match header.is_some() || hash_algorithm != HashAlgorithm::Crc {
            true => {
                let tmp_directory = create_tmp_directory(connection).await?;
                let common_romfile = self.to_common(progress_bar, &tmp_directory).await?;
                common_romfile
                    .check(connection, progress_bar, header, roms)
                    .await?;
            }
            false => {
                let (hash, size) = self
                    .get_hash_and_size(connection, progress_bar, 1, 1, &hash_algorithm)
                    .await?;
                if size != roms[0].size as u64 {
                    bail!("Size mismatch");
                };
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
            }
        }
        Ok(())
    }
}

impl ToCommon for ArchiveRomfile {
    async fn to_common<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        directory: &P,
    ) -> SimpleResult<CommonRomfile> {
        progress_bar.set_message("Extracting file");
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        progress_bar.println(format!("Extracting \"{}\"", &self.path));

        let output = Command::new(get_executable_path(SEVENZIP_EXECUTABLES)?)
            .arg("x")
            .arg(&self.romfile.path)
            .arg(self.path.replace("-", "?").replace("@", "?"))
            .current_dir(directory.as_ref())
            .output()
            .await
            .expect("Failed to extract archive");

        if !output.status.success() {
            bail!(String::from_utf8(output.stderr).unwrap().as_str())
        }

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        CommonRomfile::from_path(&directory.as_ref().join(&self.path))
    }
}

#[allow(clippy::too_many_arguments)]
pub trait ToArchive {
    async fn to_archive<P: AsRef<Path>, Q: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        working_directory: &P,
        destination_directory: &Q,
        archive_name: &str,
        archive_type: &ArchiveType,
        compression_level: &Option<usize>,
        solid: bool,
    ) -> SimpleResult<ArchiveRomfile>;
}

impl ToArchive for CommonRomfile {
    async fn to_archive<P: AsRef<Path>, Q: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        working_directory: &P,
        destination_directory: &Q,
        archive_name: &str,
        archive_type: &ArchiveType,
        compression_level: &Option<usize>,
        solid: bool,
    ) -> SimpleResult<ArchiveRomfile> {
        progress_bar.set_message(format!("Creating {}", archive_type));
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        progress_bar.println(format!("Compressing \"{}\"", &self));

        let archive_path = destination_directory.as_ref().join(format!(
            "{}.{}",
            archive_name,
            match archive_type {
                ArchiveType::Sevenzip => SEVENZIP_EXTENSION,
                ArchiveType::Zip => ZIP_EXTENSION,
            }
        ));
        let path = self.path.strip_prefix(working_directory).unwrap();

        let mut command = Command::new(get_executable_path(SEVENZIP_EXECUTABLES)?);
        command
            .arg("a")
            .arg(&archive_path)
            .arg(path)
            .current_dir(working_directory.as_ref());
        if let Some(compression_level) = compression_level {
            command.arg(format!("-mx={}", compression_level));
        }
        if solid {
            command.arg("-ms=on");
        }
        let output = command
            .output()
            .await
            .expect("Failed to add files to archive");

        if !output.status.success() {
            bail!(String::from_utf8(output.stderr).unwrap().as_str())
        }

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        Ok(ArchiveRomfile {
            romfile: CommonRomfile::from_path(&archive_path)?,
            path: path.as_os_str().to_str().unwrap().to_string(),
            archive_type: *archive_type,
        })
    }
}

impl ToArchive for ArchiveRomfile {
    async fn to_archive<P: AsRef<Path>, Q: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        source_directory: &P,
        destination_directory: &Q,
        archive_name: &str,
        archive_type: &ArchiveType,
        compression_level: &Option<usize>,
        solid: bool,
    ) -> SimpleResult<ArchiveRomfile> {
        let original_romfile = self.to_common(progress_bar, source_directory).await?;
        let archive_romfile = original_romfile
            .to_archive(
                progress_bar,
                source_directory,
                destination_directory,
                archive_name,
                archive_type,
                compression_level,
                solid,
            )
            .await?;
        original_romfile.delete(progress_bar, true).await?;
        Ok(archive_romfile)
    }
}

pub trait AsArchive {
    fn as_archive(self, rom: &Rom) -> SimpleResult<ArchiveRomfile>;
    async fn as_archives(&self, progress_bar: &ProgressBar) -> SimpleResult<Vec<ArchiveRomfile>>;
}

impl AsArchive for CommonRomfile {
    fn as_archive(self, rom: &Rom) -> SimpleResult<ArchiveRomfile> {
        let path = PathBuf::from(&self.path);
        let extension = path.extension().unwrap().to_str().unwrap();
        let archive_type = try_with!(ArchiveType::from_str(extension), "Not a valid archive");
        Ok(ArchiveRomfile {
            romfile: self,
            path: rom.name.clone(),
            archive_type,
        })
    }
    async fn as_archives(&self, progress_bar: &ProgressBar) -> SimpleResult<Vec<ArchiveRomfile>> {
        progress_bar.set_message("Parsing archive");
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        let output = Command::new(get_executable_path(SEVENZIP_EXECUTABLES)?)
            .arg("l")
            .arg("-slt")
            .arg(&self.path)
            .output()
            .await
            .expect("Failed to parse archive");

        if !output.status.success() {
            bail!(String::from_utf8(output.stderr).unwrap().as_str());
        }

        let stdout = String::from_utf8(output.stdout).unwrap();
        let paths: Vec<&str> = stdout
            .lines()
            .filter(|&line| line.starts_with("Path ="))
            .skip(1) // the first line is the archive itself
            .map(|line| line.split('=').last().unwrap().trim()) // keep only the rhs
            .collect();

        let extension = self
            .path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase();
        let archive_type = try_with!(ArchiveType::from_str(&extension), "Not a valid archive");
        let archived_romfiles: Vec<ArchiveRomfile> = paths
            .into_iter()
            .map(|path| ArchiveRomfile {
                romfile: self.clone(),
                path: path.to_string(),
                archive_type,
            })
            .collect();

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        Ok(archived_romfiles)
    }
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
    let output = try_with!(
        Command::new(get_executable_path(SEVENZIP_EXECUTABLES)?)
            .output()
            .await,
        "Failed to spawn executable"
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let version = stdout
        .lines()
        .nth(1)
        .and_then(|line| VERSION_REGEX.find(line))
        .map(|version| version.as_str().to_string())
        .unwrap_or(String::from("unknown"));

    Ok(version)
}

use super::common::*;
use super::config::*;
use super::mimetype::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use anyhow::{Context, Result};
use indicatif::ProgressBar;
use sqlx::SqliteConnection;
use std::fs::{File, OpenOptions};
use std::iter::zip;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;
use strum::{Display, EnumString};
use zip::{ZipArchive, ZipWriter};

// Whichever backend handles archives in this build. Both expose the same items,
// so nothing below this line names either of them.
#[cfg(not(feature = "sevenz"))]
use self::tool as backend;
#[cfg(feature = "sevenz")]
use super::sevenz as backend;

pub const SEVENZIP_COMPRESSION_LEVEL_RANGE: [usize; 2] = [1, 9];
pub const ZIP_COMPRESSION_LEVEL_RANGE: [usize; 2] = [1, 9];

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
    pub size: u64,
    pub crc: String,
}

impl GetRomfile for ArchiveRomfile {
    fn romfile(&self) -> &CommonRomfile {
        &self.romfile
    }
}

pub trait ArchiveFile {
    async fn rename_file(
        &self,
        progress_bar: &ProgressBar,
        new_path: &str,
    ) -> Result<ArchiveRomfile>;
    async fn delete_file(&self, progress_bar: &ProgressBar) -> Result<()>;
}

impl ArchiveFile for ArchiveRomfile {
    async fn rename_file(
        &self,
        progress_bar: &ProgressBar,
        new_path: &str,
    ) -> Result<ArchiveRomfile> {
        progress_bar.set_message("Renaming file in archive");
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));
        print_action(
            progress_bar,
            &format!("Renaming \"{}\" to \"{}\"", self.path, new_path),
        );

        backend::rename(&self.romfile.path, &self.path, new_path).await?;

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        Ok(ArchiveRomfile {
            romfile: self.romfile.clone(),
            path: new_path.to_string(),
            archive_type: self.archive_type,
            size: self.size,
            crc: self.crc.clone(),
        })
    }

    async fn delete_file(&self, progress_bar: &ProgressBar) -> Result<()> {
        progress_bar.set_message("Deleting files");
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        print_action(progress_bar, &format!("Deleting \"{}\"", self.path));

        backend::delete(&self.romfile.path, &self.path).await?;

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        if self
            .romfile
            .as_archive(progress_bar, None)
            .await?
            .is_empty()
        {
            self.romfile.delete(progress_bar, false).await?;
        }

        Ok(())
    }
}

impl Size for ArchiveRomfile {
    async fn get_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
    ) -> Result<u64> {
        if self.size > 0 {
            Ok(self.size)
        } else {
            let tmp_directory = create_tmp_directory(connection).await?;
            let size = self
                .to_common(progress_bar, &tmp_directory)
                .await?
                .get_size(connection, progress_bar)
                .await?;
            Ok(size)
        }
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
    ) -> Result<(String, u64)> {
        if hash_algorithm == &HashAlgorithm::Crc && !self.crc.is_empty() && self.size > 0 {
            Ok((self.crc.clone(), self.size))
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
    ) -> Result<()> {
        print_action(
            progress_bar,
            &format!("Checking \"{}\" ({})", self.romfile, self.path),
        );
        let tmp_directory = create_tmp_directory(connection).await?;
        let common_romfile = self.to_common(progress_bar, &tmp_directory).await?;
        common_romfile
            .check(connection, progress_bar, header, roms)
            .await?;
        Ok(())
    }
}

impl ToCommon for ArchiveRomfile {
    async fn to_common<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        directory: &P,
    ) -> Result<CommonRomfile> {
        progress_bar.set_message("Extracting file");
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        print_action(progress_bar, &format!("Extracting \"{}\"", self.path));

        backend::extract(&self.romfile.path, &self.path, directory.as_ref()).await?;

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
    ) -> Result<ArchiveRomfile>;
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
    ) -> Result<ArchiveRomfile> {
        progress_bar.set_message(format!("Creating {}", archive_type));
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        print_action(progress_bar, &format!("Compressing \"{}\"", self));

        let archive_path = destination_directory.as_ref().join(format!(
            "{}.{}",
            archive_name,
            match archive_type {
                ArchiveType::Sevenzip => SEVENZIP_EXTENSION,
                ArchiveType::Zip => ZIP_EXTENSION,
            }
        ));
        let path = self.path.strip_prefix(working_directory).unwrap();

        backend::create(
            &archive_path,
            working_directory.as_ref(),
            path,
            archive_type,
            compression_level,
            solid,
        )
        .await?;

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        Ok(ArchiveRomfile {
            romfile: CommonRomfile::from_path(&archive_path)?,
            path: path.as_os_str().to_str().unwrap().to_string(),
            archive_type: *archive_type,
            size: 0,
            crc: String::new(),
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
    ) -> Result<ArchiveRomfile> {
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
    async fn parse_archive(
        &self,
        progress_bar: &ProgressBar,
        rom: Option<&Rom>,
    ) -> Result<Vec<(String, u64, String)>>;
    async fn as_archive(
        &self,
        progress_bar: &ProgressBar,
        rom: Option<&Rom>,
    ) -> Result<Vec<ArchiveRomfile>>;
}

impl AsArchive for CommonRomfile {
    async fn parse_archive(
        &self,
        progress_bar: &ProgressBar,
        rom: Option<&Rom>,
    ) -> Result<Vec<(String, u64, String)>> {
        progress_bar.set_message("Parsing archive");
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        let entries = backend::parse(&self.path, rom.map(|rom| rom.name.as_str())).await?;

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        Ok(entries)
    }
    async fn as_archive(
        &self,
        progress_bar: &ProgressBar,
        rom: Option<&Rom>,
    ) -> Result<Vec<ArchiveRomfile>> {
        progress_bar.set_message("Parsing archive");
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        let paths_sizes_crcs = self.parse_archive(progress_bar, rom).await?;

        let extension = self
            .path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase();
        let archive_type = ArchiveType::from_str(&extension).context("Not a valid archive")?;
        let archived_romfiles: Vec<ArchiveRomfile> = paths_sizes_crcs
            .into_iter()
            .map(|(path, size, crc)| ArchiveRomfile {
                romfile: self.clone(),
                path: path.to_string(),
                archive_type,
                size,
                crc,
            })
            .collect();

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        Ok(archived_romfiles)
    }
}

pub async fn copy_files_between_archives<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    source_archive_path: &P,
    destination_archive_path: &Q,
    source_names: &[&str],
    destination_names: &[&str],
) -> Result<()> {
    progress_bar.set_message("Copying files between archives");
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    let source_archive_file =
        File::open(source_archive_path.as_ref()).expect("Failed to read archive");
    let mut source_archive = ZipArchive::new(source_archive_file).expect("Failed to open archive");

    let destination_archive_file: File;
    let mut destination_archive: ZipWriter<File>;
    if destination_archive_path.as_ref().is_file() {
        destination_archive_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(destination_archive_path.as_ref())
            .expect("Failed to open archive");
        destination_archive =
            ZipWriter::new_append(destination_archive_file).expect("Failed to open archive");
    } else {
        destination_archive_file =
            File::create(destination_archive_path.as_ref()).expect("Failed to create archive");
        destination_archive = ZipWriter::new(destination_archive_file);
    };

    for (&source_name, &destination_name) in zip(source_names, destination_names) {
        if source_name == destination_name {
            print_action(progress_bar, &format!("Copying \"{}\"", source_name));
            destination_archive
                .raw_copy_file(source_archive.by_name(source_name).unwrap())
                .expect("Failed to copy file")
        } else {
            print_action(
                progress_bar,
                &format!("Copying \"{}\" to \"{}\"", source_name, destination_name),
            );
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

/// 7-Zip is still required for writing 7z even with the native backend, so its
/// version is what `info` reports either way.
pub async fn get_version() -> Result<String> {
    tool::get_version().await
}

/// The 7-Zip backend: every archive operation by way of the external executable.
///
/// Always compiled, because the native backend still routes 7z writes here.
pub(crate) mod tool {
    use super::ArchiveType;
    use crate::util::{get_executable_path, run_tool};
    use anyhow::{Context, Result};
    #[cfg(not(feature = "sevenz"))]
    use itertools::izip;
    use regex::Regex;
    use std::path::Path;
    use std::sync::LazyLock;
    use tokio::process::Command;

    pub const SEVENZIP_EXECUTABLES: &[&str] = &["7zz", "7z"];

    static VERSION_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+\.\d+").unwrap());

    /// Reading goes to the native backend when it is compiled in.
    #[cfg(not(feature = "sevenz"))]
    pub async fn parse<P: AsRef<Path>>(
        path: P,
        name: Option<&str>,
    ) -> Result<Vec<(String, u64, String)>> {
        let mut command = Command::new(get_executable_path(SEVENZIP_EXECUTABLES)?);
        command.arg("l").arg("-slt").arg("--").arg(path.as_ref());
        if let Some(name) = name {
            command.arg(name);
        }

        let output = run_tool(&mut command).await?;

        let stdout = String::from_utf8(output.stdout).unwrap();
        let paths: Vec<String> = stdout
            .lines()
            .filter(|&line| line.starts_with("Path ="))
            .skip(1) // the first line is the archive itself
            .map(|line| line.to_string().split_off(7)) // keep only the rhs
            .collect();
        let sizes: Vec<u64> = stdout
            .lines()
            .filter(|&line| line.starts_with("Size ="))
            .map(|line| line.to_string().split_off(7).parse().unwrap()) // keep only the rhs
            .collect();
        let crcs: Vec<String> = stdout
            .lines()
            .filter(|&line| line.starts_with("CRC ="))
            .map(|line| line.to_string().split_off(6).to_lowercase()) // keep only the rhs
            .collect();

        Ok(izip!(paths, sizes, crcs).collect())
    }

    #[cfg(not(feature = "sevenz"))]
    pub async fn extract<P: AsRef<Path>, Q: AsRef<Path>>(
        path: P,
        name: &str,
        directory: Q,
    ) -> Result<()> {
        run_tool(
            Command::new(get_executable_path(SEVENZIP_EXECUTABLES)?)
                .arg("x")
                .arg("-aoa")
                .arg("--")
                .arg(path.as_ref())
                .arg(name)
                .current_dir(directory.as_ref()),
        )
        .await?;
        Ok(())
    }

    pub async fn create<P: AsRef<Path>, Q: AsRef<Path>>(
        archive_path: P,
        working_directory: Q,
        entry_path: &Path,
        _archive_type: &ArchiveType,
        compression_level: &Option<usize>,
        solid: bool,
    ) -> Result<()> {
        let mut command = Command::new(get_executable_path(SEVENZIP_EXECUTABLES)?);
        command.arg("a");
        if let Some(compression_level) = compression_level {
            command.arg(format!("-mx={}", compression_level));
        }
        if solid {
            command.arg("-ms=on");
        }
        command
            .arg("--")
            .arg(archive_path.as_ref())
            .arg(entry_path)
            .current_dir(working_directory.as_ref());
        run_tool(&mut command).await?;
        Ok(())
    }

    pub async fn rename<P: AsRef<Path>>(path: P, from: &str, to: &str) -> Result<()> {
        run_tool(
            Command::new(get_executable_path(SEVENZIP_EXECUTABLES)?)
                .arg("rn")
                .arg("--")
                .arg(path.as_ref())
                .arg(from)
                .arg(to),
        )
        .await?;
        Ok(())
    }

    pub async fn delete<P: AsRef<Path>>(path: P, name: &str) -> Result<()> {
        run_tool(
            Command::new(get_executable_path(SEVENZIP_EXECUTABLES)?)
                .arg("d")
                .arg("--")
                .arg(path.as_ref())
                .arg(name),
        )
        .await?;
        Ok(())
    }

    pub async fn get_version() -> Result<String> {
        let output = Command::new(get_executable_path(SEVENZIP_EXECUTABLES)?)
            .output()
            .await
            .context("Failed to spawn executable")?;

        let stdout = String::from_utf8(output.stdout).unwrap();
        let version = stdout
            .lines()
            .nth(1)
            .and_then(|line| VERSION_REGEX.find(line))
            .map(|version| version.as_str().to_string())
            .unwrap_or(String::from("unknown"));

        Ok(version)
    }
}

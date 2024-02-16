use super::common::*;
use super::config::*;
use super::model::*;
use super::progress::*;
use super::sevenzip::*;
use super::util::*;
use super::SimpleResult;
use indicatif::ProgressBar;
use regex::Regex;
use sqlx::SqliteConnection;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;
use strum::{Display, EnumString};
use tokio::process::Command;

const MAXCSO: &str = "maxcso";

lazy_static! {
    static ref VERSION_REGEX: Regex = Regex::new(r"\d+\.\d+\.\d+").unwrap();
}

#[derive(Clone, Copy, Display, EnumString, PartialEq, Eq)]
#[strum(serialize_all = "lowercase")]
pub enum XsoType {
    Cso,
    Zso,
}

pub struct XsoRomfile {
    pub path: PathBuf,
    pub xso_type: XsoType,
}

impl AsOriginal for XsoRomfile {
    fn as_original(&self) -> SimpleResult<CommonRomfile> {
        Ok(CommonRomfile::from_path(&self.path)?)
    }
}

impl Hash for XsoRomfile {
    async fn get_hash_and_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> simple_error::SimpleResult<(String, u64)> {
        let tmp_directory = create_tmp_directory(connection).await?;
        let original_file = self.to_original(progress_bar, &tmp_directory).await?;
        let hash_and_size = original_file
            .get_hash_and_size(
                connection,
                progress_bar,
                header,
                position,
                total,
                hash_algorithm,
            )
            .await?;
        original_file.delete(progress_bar, true).await?;
        Ok(hash_and_size)
    }
}

impl Check for XsoRomfile {
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
        let (hash, size) = self
            .get_hash_and_size(
                connection,
                progress_bar,
                header,
                position,
                total,
                hash_algorithm,
            )
            .await?;
        if size != rom.size as u64 {
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

        Ok(())
    }
}

impl ToOriginal for XsoRomfile {
    async fn to_original<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> SimpleResult<CommonRomfile> {
        progress_bar.set_message(format!("Extracting {}", self.xso_type));
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        progress_bar.println(format!(
            "Extracting \"{}\"",
            self.path.file_name().unwrap().to_str().unwrap()
        ));

        let mut path = destination_directory
            .as_ref()
            .join(self.path.file_name().unwrap());
        path.set_extension(ISO_EXTENSION);

        let output = Command::new(MAXCSO)
            .arg("--decompress")
            .arg(&self.path)
            .arg("-o")
            .arg(&path)
            .output()
            .await
            .expect(&format!("Failed to extract {}", self.xso_type));

        if !output.status.success() {
            bail!(String::from_utf8(output.stderr).unwrap().as_str())
        }

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        Ok(CommonRomfile { path })
    }
}

pub trait ToXso {
    async fn to_xso<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        xso_type: &XsoType,
    ) -> SimpleResult<XsoRomfile>;
}

impl ToXso for CommonRomfile {
    async fn to_xso<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        xso_type: &XsoType,
    ) -> SimpleResult<XsoRomfile> {
        progress_bar.set_message(format!("Creating {}", xso_type));
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        let mut path = destination_directory
            .as_ref()
            .join(self.path.file_name().unwrap());
        path.set_extension(match xso_type {
            XsoType::Cso => CSO_EXTENSION,
            XsoType::Zso => ZSO_EXTENSION,
        });

        progress_bar.println(format!(
            "Creating \"{}\"",
            path.file_name().unwrap().to_str().unwrap()
        ));

        let output = Command::new(MAXCSO)
            .arg("--block=2048")
            .arg(format!(
                "--format={}",
                match xso_type {
                    XsoType::Cso => "cso1",
                    XsoType::Zso => "zso",
                }
            ))
            .arg(&self.path)
            .arg("-o")
            .arg(&path)
            .output()
            .await
            .expect(&format!("Failed to create {}", xso_type));

        if !output.status.success() {
            bail!(String::from_utf8(output.stderr).unwrap().as_str())
        }

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        Ok(XsoRomfile {
            path,
            xso_type: xso_type.clone(),
        })
    }
}

impl ToArchive for XsoRomfile {
    async fn to_archive<P: AsRef<Path>, Q: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        working_directory: &P,
        destination_directory: &Q,
        archive_name: &str,
        archive_type: &ArchiveType,
        compression_level: usize,
        solid: bool,
    ) -> SimpleResult<ArchiveRomfile> {
        let original_file = self.to_original(progress_bar, working_directory).await?;
        let archive_romfile = original_file
            .to_archive(
                progress_bar,
                working_directory,
                destination_directory,
                archive_name,
                archive_type,
                compression_level,
                solid,
            )
            .await?;
        Ok(archive_romfile)
    }
}

impl FromPath<XsoRomfile> for XsoRomfile {
    fn from_path<P: AsRef<Path>>(path: &P) -> SimpleResult<XsoRomfile> {
        let path = path.as_ref().to_path_buf();
        let extension = path.extension().unwrap().to_str().unwrap().to_lowercase();
        let xso_type = try_with!(XsoType::from_str(&extension), "Not a valid xso");
        Ok(XsoRomfile { path, xso_type })
    }
}

pub trait AsXso {
    fn as_xso(&self) -> SimpleResult<XsoRomfile>;
}

impl AsXso for Romfile {
    fn as_xso(&self) -> SimpleResult<XsoRomfile> {
        Ok(XsoRomfile::from_path(&self.path)?)
    }
}

pub async fn get_version() -> SimpleResult<String> {
    let output = try_with!(
        Command::new(MAXCSO).output().await,
        "Failed to spawn maxcso"
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    let version = stderr
        .lines()
        .next()
        .map(|line| VERSION_REGEX.find(line))
        .flatten()
        .map(|version| version.as_str().to_string())
        .unwrap_or(String::from("unknown"));

    Ok(version)
}

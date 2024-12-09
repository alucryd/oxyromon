use super::common::*;
use super::config::*;
use super::mimetype::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use super::SimpleResult;
use indicatif::ProgressBar;
use regex::Regex;
use sqlx::SqliteConnection;
use std::path::Path;
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
    pub romfile: CommonRomfile,
    pub xso_type: XsoType,
}

impl HashAndSize for XsoRomfile {
    async fn get_hash_and_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> simple_error::SimpleResult<(String, u64)> {
        let tmp_directory = create_tmp_directory(connection).await?;
        let iso_romfile = self.to_iso(progress_bar, &tmp_directory).await?;
        let (hash, size) = iso_romfile
            .romfile
            .get_hash_and_size(connection, progress_bar, position, total, hash_algorithm)
            .await?;
        iso_romfile.romfile.delete(progress_bar, true).await?;
        Ok((hash, size))
    }
}

impl Check for XsoRomfile {
    async fn check(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        roms: &[&Rom],
        hash_algorithm: &HashAlgorithm,
    ) -> SimpleResult<()> {
        progress_bar.println(format!("Checking \"{}\"", self.romfile));
        let tmp_directory = create_tmp_directory(connection).await?;
        let iso_romfile = self.to_iso(progress_bar, &tmp_directory).await?;
        iso_romfile
            .romfile
            .check(connection, progress_bar, header, roms, hash_algorithm)
            .await?;
        Ok(())
    }
}

impl ToIso for XsoRomfile {
    async fn to_iso<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> SimpleResult<IsoRomfile> {
        progress_bar.set_message(format!("Extracting {}", self.xso_type));
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        progress_bar.println(format!(
            "Extracting \"{}\"",
            self.romfile.path.file_name().unwrap().to_str().unwrap()
        ));

        let path = destination_directory
            .as_ref()
            .join(self.romfile.path.file_name().unwrap())
            .with_extension(ISO_EXTENSION);

        let output = Command::new(MAXCSO)
            .arg("--decompress")
            .arg(&self.romfile.path)
            .arg("-o")
            .arg(&path)
            .output()
            .await
            .unwrap_or_else(|_| panic!("Failed to extract {}", self.xso_type));

        if !output.status.success() {
            bail!(String::from_utf8(output.stderr).unwrap().as_str())
        }

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        CommonRomfile::from_path(&path)?.as_iso()
    }
}

pub trait ToXso {
    async fn to_xso<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        xso_type: XsoType,
    ) -> SimpleResult<XsoRomfile>;
}

impl ToXso for IsoRomfile {
    async fn to_xso<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        xso_type: XsoType,
    ) -> SimpleResult<XsoRomfile> {
        progress_bar.set_message(format!("Creating {}", xso_type));
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        let path = destination_directory
            .as_ref()
            .join(self.romfile.path.file_name().unwrap())
            .with_extension(match xso_type {
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
            .arg(&self.romfile.path)
            .arg("-o")
            .arg(&path)
            .output()
            .await
            .unwrap_or_else(|_| panic!("Failed to create {}", xso_type));

        if !output.status.success() {
            bail!(String::from_utf8(output.stderr).unwrap().as_str())
        }

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        CommonRomfile::from_path(&path)?.as_xso().await
    }
}

pub trait AsXso {
    async fn as_xso(self) -> SimpleResult<XsoRomfile>;
}

impl AsXso for CommonRomfile {
    async fn as_xso(self) -> SimpleResult<XsoRomfile> {
        let mimetype = get_mimetype(&self.path).await?;
        if mimetype.is_none() {
            bail!("Not a valid xso");
        }
        let xso_type = try_with!(
            XsoType::from_str(mimetype.unwrap().extension()),
            "Not a valid xso"
        );
        Ok(XsoRomfile {
            romfile: self,
            xso_type,
        })
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
        .and_then(|line| VERSION_REGEX.find(line))
        .map(|version| version.as_str().to_string())
        .unwrap_or(String::from("unknown"));

    Ok(version)
}

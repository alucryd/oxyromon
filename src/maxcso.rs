use super::common::*;
use super::config::*;
use super::mimetype::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use anyhow::{Context, Result, bail};
use indicatif::ProgressBar;
use regex::Regex;
use sqlx::SqliteConnection;
use std::path::Path;
use std::str::FromStr;
use std::sync::LazyLock;
use std::time::Duration;
use strum::{Display, EnumString};
use tokio::process::Command;

const MAXCSO: &str = "maxcso";

static VERSION_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+\.\d+\.\d+").unwrap());

#[derive(Clone, Copy, Display, EnumString, PartialEq, Eq)]
#[strum(serialize_all = "lowercase")]
pub enum XsoType {
    Cso,
    Zso,
}

impl XsoType {
    pub fn extension(&self) -> &'static str {
        match self {
            XsoType::Cso => CSO_EXTENSION,
            XsoType::Zso => ZSO_EXTENSION,
        }
    }

    pub fn opposite(&self) -> XsoType {
        match self {
            XsoType::Cso => XsoType::Zso,
            XsoType::Zso => XsoType::Cso,
        }
    }
}

pub struct XsoRomfile {
    pub romfile: CommonRomfile,
    pub xso_type: XsoType,
}

impl GetRomfile for XsoRomfile {
    fn romfile(&self) -> &CommonRomfile {
        &self.romfile
    }
}

impl HashAndSize for XsoRomfile {
    async fn get_hash_and_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> Result<(String, u64)> {
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
    ) -> Result<()> {
        print_action(progress_bar, &format!("Checking \"{}\"", self.romfile));
        let tmp_directory = create_tmp_directory(connection).await?;
        let iso_romfile = self.to_iso(progress_bar, &tmp_directory).await?;
        iso_romfile
            .romfile
            .check(connection, progress_bar, header, roms)
            .await?;
        Ok(())
    }
}

impl ToIso for XsoRomfile {
    async fn to_iso<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<IsoRomfile> {
        progress_bar.set_message(format!("Extracting {}", self.xso_type));
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        print_action(
            progress_bar,
            &format!(
                "Extracting \"{}\"",
                self.romfile.path.file_name().unwrap().to_str().unwrap()
            ),
        );

        let path = destination_directory
            .as_ref()
            .join(self.romfile.path.file_name().unwrap())
            .with_extension(ISO_EXTENSION);

        run_tool(
            Command::new(MAXCSO)
                .arg("--decompress")
                .arg(&self.romfile.path)
                .arg("-o")
                .arg(&path),
        )
        .await?;

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
    ) -> Result<XsoRomfile>;
}

impl ToXso for IsoRomfile {
    async fn to_xso<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        xso_type: XsoType,
    ) -> Result<XsoRomfile> {
        progress_bar.set_message(format!("Creating {}", xso_type));
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        let path = destination_directory
            .as_ref()
            .join(self.romfile.path.file_name().unwrap())
            .with_extension(xso_type.extension());

        print_action(
            progress_bar,
            &format!(
                "Creating \"{}\"",
                path.file_name().unwrap().to_str().unwrap()
            ),
        );

        run_tool(
            Command::new(MAXCSO)
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
                .arg(&path),
        )
        .await?;

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        CommonRomfile::from_path(&path)?.as_xso().await
    }
}

pub trait AsXso {
    async fn as_xso(self) -> Result<XsoRomfile>;
}

impl AsXso for CommonRomfile {
    async fn as_xso(self) -> Result<XsoRomfile> {
        let mimetype = get_mimetype(&self.path).await?;
        if mimetype.is_none() {
            bail!("Not a valid xso");
        }
        let xso_type =
            XsoType::from_str(mimetype.unwrap().extension()).context("Not a valid xso")?;
        Ok(XsoRomfile {
            romfile: self,
            xso_type,
        })
    }
}

pub async fn get_version() -> Result<String> {
    let output = Command::new(MAXCSO)
        .output()
        .await
        .context("Failed to spawn maxcso")?;

    let stderr = String::from_utf8(output.stderr).unwrap();
    let version = stderr
        .lines()
        .next()
        .and_then(|line| VERSION_REGEX.find(line))
        .map(|version| version.as_str().to_string())
        .unwrap_or(String::from("unknown"));

    Ok(version)
}

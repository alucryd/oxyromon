//! CSO and ZSO, the compressed ISOs PSP and PS2 loaders read, handled by the
//! xso-rs crate, a port of maxcso.

use super::common::*;
use super::config::*;
use super::mimetype::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use anyhow::{Context, Result, bail};
use indicatif::ProgressBar;
use sqlx::SqliteConnection;
use std::path::Path;
use std::str::FromStr;
use strum::{Display, EnumString};
use xso_rs::{CompressOptions, DecompressOptions, Format};

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
        start_action(progress_bar, Some(&format!("Extracting {}", self.xso_type)));

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

        let (input, output) = (self.romfile.path.clone(), path.clone());
        run_blocking(progress_bar, input.metadata()?.len(), move |progress| {
            xso_rs::decompress(&input, &output, &DecompressOptions::default(), progress)
        })
        .await
        .with_context(|| format!("Failed to extract \"{}\"", self.romfile.path.display()))?;

        stop_action(progress_bar);

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
        start_action(progress_bar, Some(&format!("Creating {}", xso_type)));

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

        // xso-rs picks the block size: 8 KiB for CSO, which an ARK-5 PSP plays,
        // or 16 KiB from 2 GiB, where only PS2 DVDs are; 2 KiB for ZSO, the only
        // size Open PS2 Loader reads.
        let options = CompressOptions::new(match xso_type {
            XsoType::Cso => Format::Cso,
            XsoType::Zso => Format::Zso,
        });
        let (input, output) = (self.romfile.path.clone(), path.clone());
        run_blocking(progress_bar, input.metadata()?.len(), move |progress| {
            xso_rs::compress(&input, &output, &options, progress)
        })
        .await
        .with_context(|| format!("Failed to create \"{}\"", path.display()))?;

        stop_action(progress_bar);

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

/// Reported by `info`. Like nod, a linked library has no version to query at
/// runtime.
pub async fn get_version() -> Result<String> {
    Ok(String::from("built-in"))
}

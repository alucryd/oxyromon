use super::common::*;
use super::config::*;
use super::mimetype::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use anyhow::{Result, bail};
use indicatif::ProgressBar;
use sqlx::SqliteConnection;
use std::path::Path;
use tokio::process::Command;

const NSZ: &str = "nsz";

pub struct NspRomfile {
    pub romfile: CommonRomfile,
}

impl GetRomfile for NspRomfile {
    fn romfile(&self) -> &CommonRomfile {
        &self.romfile
    }
}

pub struct NszRomfile {
    pub romfile: CommonRomfile,
}

impl GetRomfile for NszRomfile {
    fn romfile(&self) -> &CommonRomfile {
        &self.romfile
    }
}

impl HashAndSize for NszRomfile {
    async fn get_hash_and_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> Result<(String, u64)> {
        let tmp_directory = create_tmp_directory(connection).await?;
        let nsp_romfile = self.to_nsp(progress_bar, &tmp_directory).await?;
        let (hash, size) = nsp_romfile
            .romfile
            .get_hash_and_size(connection, progress_bar, position, total, hash_algorithm)
            .await?;
        nsp_romfile.romfile.delete(progress_bar, true).await?;
        Ok((hash, size))
    }
}

impl Check for NszRomfile {
    async fn check(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        roms: &[&Rom],
    ) -> Result<()> {
        print_action(progress_bar, &format!("Checking \"{}\"", self.romfile));
        let tmp_directory = create_tmp_directory(connection).await?;
        let nsp_romfile = self.to_nsp(progress_bar, &tmp_directory).await?;
        nsp_romfile
            .romfile
            .check(connection, progress_bar, header, roms)
            .await?;
        Ok(())
    }
}

pub trait ToNsp {
    async fn to_nsp<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<NspRomfile>;
}

impl ToNsp for NszRomfile {
    async fn to_nsp<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<NspRomfile> {
        start_action(progress_bar, Some("Extracting nsz"));

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
            .with_extension(NSP_EXTENSION);

        run_tool(
            Command::new(NSZ)
                .arg("-D")
                .arg("-F")
                .arg("-o")
                .arg(destination_directory.as_ref())
                .arg(&self.romfile.path),
        )
        .await?;

        stop_action(progress_bar);

        CommonRomfile::from_path(&path)?.as_nsp()
    }
}

pub trait ToNsz {
    async fn to_nsz<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<NszRomfile>;
}

impl ToNsz for NspRomfile {
    async fn to_nsz<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<NszRomfile> {
        start_action(progress_bar, Some("Creating nsz"));

        let path = destination_directory
            .as_ref()
            .join(self.romfile.path.file_name().unwrap())
            .with_extension(NSZ_EXTENSION);

        print_action(
            progress_bar,
            &format!(
                "Creating \"{}\"",
                path.file_name().unwrap().to_str().unwrap()
            ),
        );

        run_tool(
            Command::new(NSZ)
                .arg("-C")
                .arg("-K")
                .arg("-L")
                .arg("-P")
                .arg("-o")
                .arg(destination_directory.as_ref())
                .arg(&self.romfile.path),
        )
        .await?;

        stop_action(progress_bar);

        CommonRomfile::from_path(&path)?.as_nsz()
    }
}

pub trait AsNsp {
    fn as_nsp(self) -> Result<NspRomfile>;
}

impl AsNsp for CommonRomfile {
    fn as_nsp(self) -> Result<NspRomfile> {
        if self
            .path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase()
            != NSP_EXTENSION
        {
            bail!("Not a valid nsp");
        }
        Ok(NspRomfile { romfile: self })
    }
}

pub trait AsNsz {
    fn as_nsz(self) -> Result<NszRomfile>;
}

impl AsNsz for CommonRomfile {
    fn as_nsz(self) -> Result<NszRomfile> {
        if self
            .path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase()
            != NSZ_EXTENSION
        {
            bail!("Not a valid nsz");
        }
        Ok(NszRomfile { romfile: self })
    }
}

pub async fn get_version() -> Result<String> {
    let keys_path = dirs::home_dir().map(|home| home.join(".switch").join("prod.keys"));
    if keys_path.map(|p| p.is_file()) != Some(true) {
        bail!("prod.keys not found");
    }

    tool_version(NSZ, "nsz", &["-h"], false, 0, None).await
}

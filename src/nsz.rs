use super::common::*;
use super::config::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use super::SimpleResult;
use indicatif::ProgressBar;
use regex::Regex;
use sqlx::SqliteConnection;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

const NSZ: &str = "nsz";

pub struct NspRomfile {
    pub path: PathBuf,
}

impl AsCommon for NspRomfile {
    fn as_common(&self) -> SimpleResult<CommonRomfile> {
        CommonRomfile::from_path(&self.path)
    }
}

pub struct NszRomfile {
    pub path: PathBuf,
}

impl AsCommon for NszRomfile {
    fn as_common(&self) -> SimpleResult<CommonRomfile> {
        CommonRomfile::from_path(&self.path)
    }
}

impl Hash for NszRomfile {
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
        let nsp_romfile = self.to_nsp(progress_bar, &tmp_directory).await?;
        let hash_and_size = nsp_romfile
            .as_common()?
            .get_hash_and_size(
                connection,
                progress_bar,
                header,
                position,
                total,
                hash_algorithm,
            )
            .await?;
        nsp_romfile.as_common()?.delete(progress_bar, true).await?;
        Ok(hash_and_size)
    }
}

impl Check for NszRomfile {
    async fn check(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        roms: &[&Rom],
        hash_algorithm: &HashAlgorithm,
    ) -> SimpleResult<()> {
        progress_bar.println(format!("Checking \"{}\"", self.as_common()?));
        let tmp_directory = create_tmp_directory(connection).await?;
        let nsp_romfile = self.to_nsp(progress_bar, &tmp_directory).await?;
        nsp_romfile
            .as_common()?
            .check(connection, progress_bar, header, roms, hash_algorithm)
            .await?;
        Ok(())
    }
}

pub trait ToNsp {
    async fn to_nsp<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> SimpleResult<NspRomfile>;
}

impl ToNsp for NszRomfile {
    async fn to_nsp<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> SimpleResult<NspRomfile> {
        progress_bar.set_message("Extracting nsz");
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        progress_bar.println(format!(
            "Extracting \"{}\"",
            self.path.file_name().unwrap().to_str().unwrap()
        ));

        let path = destination_directory
            .as_ref()
            .join(self.path.file_name().unwrap())
            .with_extension(NSP_EXTENSION);

        let output = Command::new(NSZ)
            .arg("-D")
            .arg("-F")
            .arg("-o")
            .arg(destination_directory.as_ref())
            .arg(&self.path)
            .output()
            .await
            .expect("Failed to extract nsz");

        if !output.status.success() {
            bail!(String::from_utf8(output.stderr).unwrap().as_str())
        }

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        Ok(NspRomfile { path })
    }
}

pub trait ToNsz {
    async fn to_nsz<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> SimpleResult<NszRomfile>;
}

impl ToNsz for NspRomfile {
    async fn to_nsz<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> SimpleResult<NszRomfile> {
        progress_bar.set_message("Creating nsz");
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        let path = destination_directory
            .as_ref()
            .join(self.path.file_name().unwrap())
            .with_extension(NSZ_EXTENSION);

        progress_bar.println(format!(
            "Creating \"{}\"",
            path.file_name().unwrap().to_str().unwrap()
        ));

        let output = Command::new(NSZ)
            .arg("-C")
            .arg("-K")
            .arg("-L")
            .arg("-P")
            .arg("-o")
            .arg(destination_directory.as_ref())
            .arg(&self.path)
            .output()
            .await
            .expect("Failed to create nsz");

        if !output.status.success() {
            bail!(String::from_utf8(output.stderr).unwrap().as_str())
        }

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        Ok(NszRomfile { path })
    }
}

impl FromPath<NspRomfile> for NspRomfile {
    fn from_path<P: AsRef<Path>>(path: &P) -> SimpleResult<NspRomfile> {
        let path = path.as_ref().to_path_buf();
        let extension = path.extension().unwrap().to_str().unwrap().to_lowercase();
        if extension != NSP_EXTENSION {
            bail!("Not a valid rvz");
        }
        Ok(NspRomfile { path })
    }
}

impl FromPath<NszRomfile> for NszRomfile {
    fn from_path<P: AsRef<Path>>(path: &P) -> SimpleResult<NszRomfile> {
        let path = path.as_ref().to_path_buf();
        let extension = path.extension().unwrap().to_str().unwrap().to_lowercase();
        if extension != NSZ_EXTENSION {
            bail!("Not a valid rvz");
        }
        Ok(NszRomfile { path })
    }
}

pub trait AsNsp {
    fn as_nsp(&self) -> SimpleResult<NspRomfile>;
}

impl AsNsp for Romfile {
    fn as_nsp(&self) -> SimpleResult<NspRomfile> {
        NspRomfile::from_path(&self.path)
    }
}

impl AsNsp for CommonRomfile {
    fn as_nsp(&self) -> SimpleResult<NspRomfile> {
        NspRomfile::from_path(&self.path)
    }
}

pub trait AsNsz {
    fn as_nsz(&self) -> SimpleResult<NszRomfile>;
}

impl AsNsz for Romfile {
    fn as_nsz(&self) -> SimpleResult<NszRomfile> {
        NszRomfile::from_path(&self.path)
    }
}

pub async fn get_version() -> SimpleResult<String> {
    let output = try_with!(
        Command::new(NSZ).arg("-h").output().await,
        "Failed to spawn nsz"
    );

    // nsz doesn't advertise any version
    String::from_utf8(output.stderr).unwrap();
    let version = String::from("unknown");

    Ok(version)
}

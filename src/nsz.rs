use super::SimpleResult;
use super::common::*;
use super::config::*;
use super::mimetype::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use indicatif::ProgressBar;
use sqlx::SqliteConnection;
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;

const NSZ: &str = "nsz";

pub struct NspRomfile {
    pub romfile: CommonRomfile,
}

pub struct NszRomfile {
    pub romfile: CommonRomfile,
}

impl HashAndSize for NszRomfile {
    async fn get_hash_and_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> simple_error::SimpleResult<(String, u64)> {
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
    ) -> SimpleResult<()> {
        progress_bar.println(format!("Checking \"{}\"", self.romfile));
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
            self.romfile.path.file_name().unwrap().to_str().unwrap()
        ));

        let path = destination_directory
            .as_ref()
            .join(self.romfile.path.file_name().unwrap())
            .with_extension(NSP_EXTENSION);

        let output = Command::new(NSZ)
            .arg("-D")
            .arg("-F")
            .arg("-o")
            .arg(destination_directory.as_ref())
            .arg(&self.romfile.path)
            .output()
            .await
            .expect("Failed to extract nsz");

        if !output.status.success() {
            bail!(String::from_utf8(output.stderr).unwrap().as_str())
        }

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        CommonRomfile::from_path(&path)?.as_nsp()
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
            .join(self.romfile.path.file_name().unwrap())
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
            .arg(&self.romfile.path)
            .output()
            .await
            .expect("Failed to create nsz");

        if !output.status.success() {
            bail!(String::from_utf8(output.stderr).unwrap().as_str())
        }

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        CommonRomfile::from_path(&path)?.as_nsz()
    }
}

pub trait AsNsp {
    fn as_nsp(self) -> SimpleResult<NspRomfile>;
}

impl AsNsp for CommonRomfile {
    fn as_nsp(self) -> SimpleResult<NspRomfile> {
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
    fn as_nsz(self) -> SimpleResult<NszRomfile>;
}

impl AsNsz for CommonRomfile {
    fn as_nsz(self) -> SimpleResult<NszRomfile> {
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

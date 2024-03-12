use super::common::*;
use super::config::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use super::SimpleResult;
use cfg_if::cfg_if;
use indicatif::ProgressBar;
use sqlx::SqliteConnection;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

cfg_if! {
    if #[cfg(windows)] {
        const DOLPHIN_TOOL: &str = "DolphinTool.exe";
    } else {
        const DOLPHIN_TOOL: &str = "dolphin-tool";
    }
}

pub const RVZ_BLOCK_SIZE_RANGE: [usize; 2] = [32, 2048];
pub const RVZ_COMPRESSION_LEVEL_RANGE: [usize; 2] = [1, 22];

pub struct RvzRomfile {
    pub path: PathBuf,
}

impl AsCommon for RvzRomfile {
    fn as_common(&self) -> SimpleResult<CommonRomfile> {
        CommonRomfile::from_path(&self.path)
    }
}

impl Hash for RvzRomfile {
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
        let iso_romfile = self.to_iso(progress_bar, &tmp_directory).await?;
        let hash_and_size = iso_romfile
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
        iso_romfile.as_common()?.delete(progress_bar, true).await?;
        Ok(hash_and_size)
    }
}

impl Check for RvzRomfile {
    async fn check(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        roms: &[&Rom],
        hash_algorithm: &HashAlgorithm,
    ) -> SimpleResult<()> {
        progress_bar.println(format!("Checking \"{}\"", self.as_common()?.to_string()));
        let tmp_directory = create_tmp_directory(connection).await?;
        let iso_romfile = self.to_iso(progress_bar, &tmp_directory.path()).await?;
        iso_romfile
            .as_common()?
            .check(connection, progress_bar, header, roms, hash_algorithm)
            .await?;
        Ok(())
    }
}

impl ToIso for RvzRomfile {
    async fn to_iso<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> SimpleResult<IsoRomfile> {
        progress_bar.set_message("Extracting rvz");
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        progress_bar.println(format!(
            "Extracting \"{}\"",
            self.path.file_name().unwrap().to_str().unwrap()
        ));

        let path = destination_directory
            .as_ref()
            .join(self.path.file_name().unwrap())
            .with_extension(ISO_EXTENSION);

        let output = Command::new(DOLPHIN_TOOL)
            .arg("convert")
            .arg("-f")
            .arg("iso")
            .arg("-i")
            .arg(&self.path)
            .arg("-o")
            .arg(&path)
            .output()
            .await
            .expect("Failed to extract rvz");

        if !output.status.success() {
            bail!(String::from_utf8(output.stderr).unwrap().as_str())
        }

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        Ok(IsoRomfile { path })
    }
}

pub trait ToRvz {
    async fn to_rvz<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        compression_algorithm: &RvzCompressionAlgorithm,
        compression_level: usize,
        block_size: usize,
    ) -> SimpleResult<RvzRomfile>;
}

impl ToRvz for IsoRomfile {
    async fn to_rvz<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        compression_algorithm: &RvzCompressionAlgorithm,
        compression_level: usize,
        block_size: usize,
    ) -> SimpleResult<RvzRomfile> {
        progress_bar.set_message("Creating rvz");
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        let path = destination_directory
            .as_ref()
            .join(self.path.file_name().unwrap())
            .with_extension(RVZ_EXTENSION);

        progress_bar.println(format!(
            "Creating \"{}\"",
            path.file_name().unwrap().to_str().unwrap()
        ));

        let output = Command::new(DOLPHIN_TOOL)
            .arg("convert")
            .arg("-f")
            .arg("rvz")
            .arg("-c")
            .arg(compression_algorithm.to_string())
            .arg("-l")
            .arg(compression_level.to_string())
            .arg("-b")
            .arg((block_size * 1024).to_string())
            .arg("-i")
            .arg(&self.path)
            .arg("-o")
            .arg(&path)
            .output()
            .await
            .expect("Failed to create rvz");

        if !output.status.success() {
            bail!(String::from_utf8(output.stderr).unwrap().as_str())
        }

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        Ok(RvzRomfile { path })
    }
}

impl FromPath<RvzRomfile> for RvzRomfile {
    fn from_path<P: AsRef<Path>>(path: &P) -> SimpleResult<RvzRomfile> {
        let path = path.as_ref().to_path_buf();
        let extension = path.extension().unwrap().to_str().unwrap().to_lowercase();
        if extension != RVZ_EXTENSION {
            bail!("Not a valid rvz");
        }
        Ok(RvzRomfile { path })
    }
}

pub trait AsRvz {
    fn as_rvz(&self) -> SimpleResult<RvzRomfile>;
}

impl AsRvz for Romfile {
    fn as_rvz(&self) -> SimpleResult<RvzRomfile> {
        RvzRomfile::from_path(&self.path)
    }
}

pub async fn get_version() -> SimpleResult<String> {
    let output = try_with!(
        Command::new(DOLPHIN_TOOL).output().await,
        "Failed to spawn dolphin"
    );
    // dolphin doesn't advertize any version
    String::from_utf8(output.stderr).unwrap();
    let version = String::from("unknown");
    Ok(version)
}

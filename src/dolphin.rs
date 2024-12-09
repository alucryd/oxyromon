use super::common::*;
use super::config::*;
use super::mimetype::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use super::SimpleResult;
use indicatif::ProgressBar;
use sqlx::SqliteConnection;
use std::path::Path;
use std::time::Duration;
use strum::{Display, EnumString, VariantNames};
use tokio::process::Command;

pub const DOLPHIN_TOOL_EXECUTABLES: &[&str] = &["dolphin-tool", "DolphinTool"];
pub const RVZ_BLOCK_SIZE_RANGE: [usize; 2] = [32, 2048];
pub const RVZ_COMPRESSION_LEVEL_RANGE: [usize; 2] = [1, 22];

#[derive(Display, PartialEq, EnumString, VariantNames)]
#[strum(serialize_all = "lowercase")]
pub enum RvzCompressionAlgorithm {
    None,
    Zstd,
    Bzip,
    Lzma,
    Lzma2,
}

pub struct RvzRomfile {
    pub romfile: CommonRomfile,
}

impl HashAndSize for RvzRomfile {
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

impl Check for RvzRomfile {
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
        let iso_romfile = self.to_iso(progress_bar, &tmp_directory.path()).await?;
        iso_romfile
            .romfile
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
            self.romfile.path.file_name().unwrap().to_str().unwrap()
        ));

        let path = destination_directory
            .as_ref()
            .join(self.romfile.path.file_name().unwrap())
            .with_extension(ISO_EXTENSION);

        let output = Command::new(get_executable_path(DOLPHIN_TOOL_EXECUTABLES)?)
            .arg("convert")
            .arg("-f")
            .arg("iso")
            .arg("-i")
            .arg(&self.romfile.path)
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

        CommonRomfile::from_path(&path)?.as_iso()
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
        scrub: bool,
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
        scrub: bool,
    ) -> SimpleResult<RvzRomfile> {
        progress_bar.set_message("Creating rvz");
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        let path = destination_directory
            .as_ref()
            .join(self.romfile.path.file_name().unwrap())
            .with_extension(RVZ_EXTENSION);

        progress_bar.println(format!(
            "Creating \"{}\"",
            path.file_name().unwrap().to_str().unwrap()
        ));

        let mut command = Command::new(get_executable_path(DOLPHIN_TOOL_EXECUTABLES)?);
        command
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
            .arg(&self.romfile.path)
            .arg("-o")
            .arg(&path);
        if scrub {
            command.arg("-s");
        }
        let output = command.output().await.expect("Failed to create rvz");

        if !output.status.success() {
            bail!(String::from_utf8(output.stderr).unwrap().as_str())
        }

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        CommonRomfile::from_path(&path)?.as_rvz()
    }
}

pub trait AsRvz {
    fn as_rvz(self) -> SimpleResult<RvzRomfile>;
}

impl AsRvz for CommonRomfile {
    fn as_rvz(self) -> SimpleResult<RvzRomfile> {
        if self
            .path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase()
            != RVZ_EXTENSION
        {
            bail!("Not a valid rvz");
        }
        Ok(RvzRomfile { romfile: self })
    }
}

pub async fn get_version() -> SimpleResult<String> {
    let output = try_with!(
        Command::new(get_executable_path(DOLPHIN_TOOL_EXECUTABLES)?)
            .output()
            .await,
        "Failed to spawn dolphin-tool"
    );
    // dolphin-tool doesn't advertize any version
    String::from_utf8(output.stderr).unwrap();
    let version = String::from("unknown");
    Ok(version)
}

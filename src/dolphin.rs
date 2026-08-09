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
use strum::{Display, EnumString, VariantNames};

// Whichever backend handles RVZ in this build. Both expose the same items, so
// nothing below this line names either of them.
#[cfg(not(feature = "nod"))]
use self::tool as backend;
#[cfg(feature = "nod")]
use super::nod as backend;

pub use backend::BACKEND_NAME;

pub const RVZ_BLOCK_SIZE_RANGE: [usize; 2] = [32, 2048];
pub const RVZ_COMPRESSION_LEVEL_RANGE: [usize; 2] = [1, 22];

#[derive(Display, PartialEq, EnumString, VariantNames)]
#[strum(serialize_all = "lowercase")]
pub enum RvzCompressionAlgorithm {
    None,
    Zstd,
    Bzip2,
    Lzma,
    Lzma2,
}

pub struct RvzRomfile {
    pub romfile: CommonRomfile,
}

impl GetRomfile for RvzRomfile {
    fn romfile(&self) -> &CommonRomfile {
        &self.romfile
    }
}

impl HashAndSize for RvzRomfile {
    async fn get_hash_and_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> Result<(String, u64)> {
        progress_bar.reset();
        progress_bar.set_message(format!(
            "Computing {} ({}/{})",
            hash_algorithm, position, total
        ));

        // A backend that decodes in process can feed the digest directly,
        // saving a full write and read of the extracted ISO
        #[cfg(feature = "nod")]
        let (hash, size) = {
            // Nothing to extract, so no temp directory to look up
            let _ = connection;
            backend::hash(&self.romfile.path, progress_bar, hash_algorithm).await?
        };
        #[cfg(not(feature = "nod"))]
        let (hash, size) = {
            let tmp_directory = create_tmp_directory(connection).await?;
            let iso_romfile = self.to_iso(progress_bar, &tmp_directory).await?;
            let hash_and_size = iso_romfile
                .romfile
                .get_hash_and_size(connection, progress_bar, position, total, hash_algorithm)
                .await?;
            iso_romfile.romfile.delete(progress_bar, true).await?;
            hash_and_size
        };

        progress_bar.set_message("");

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
    ) -> Result<()> {
        print_action(progress_bar, &format!("Checking \"{}\"", self.romfile));

        // Headers are a cartridge era concern and no GameCube or Wii DAT
        // declares one, but honour it the long way round if one ever shows up
        if header.is_none() {
            let rom = roms[0];
            let hash_algorithm = get_hash_algorithm(rom)?;
            let (hash, size) = self
                .get_hash_and_size(connection, progress_bar, 1, 1, &hash_algorithm)
                .await?;
            return compare_hash_and_size(rom, &hash, size, &hash_algorithm);
        }

        let tmp_directory = create_tmp_directory(connection).await?;
        let iso_romfile = self.to_iso(progress_bar, &tmp_directory.path()).await?;
        iso_romfile
            .romfile
            .check(connection, progress_bar, header, roms)
            .await?;
        Ok(())
    }
}

impl ToIso for RvzRomfile {
    async fn to_iso<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<IsoRomfile> {
        progress_bar.set_message("Extracting rvz");

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

        backend::to_iso(&self.romfile.path, &path, progress_bar).await?;

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
    ) -> Result<RvzRomfile>;
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
    ) -> Result<RvzRomfile> {
        progress_bar.set_message("Creating rvz");

        let path = destination_directory
            .as_ref()
            .join(self.romfile.path.file_name().unwrap())
            .with_extension(RVZ_EXTENSION);

        print_action(
            progress_bar,
            &format!(
                "Creating \"{}\"",
                path.file_name().unwrap().to_str().unwrap()
            ),
        );

        // Say so rather than quietly handing back an unscrubbed image
        if scrub && !backend::SUPPORTS_SCRUB {
            print_warning(
                progress_bar,
                "RVZ_SCRUB is unsupported by this build's backend, writing unscrubbed",
            );
        }

        backend::to_rvz(
            &self.romfile.path,
            &path,
            progress_bar,
            compression_algorithm,
            compression_level,
            block_size,
            scrub,
        )
        .await?;

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        CommonRomfile::from_path(&path)?.as_rvz()
    }
}

pub trait AsRvz {
    fn as_rvz(self) -> Result<RvzRomfile>;
}

impl AsRvz for CommonRomfile {
    fn as_rvz(self) -> Result<RvzRomfile> {
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

pub async fn get_version() -> Result<String> {
    backend::get_version().await
}

/// The dolphin-tool backend: RVZ by way of the external executable.
#[cfg(not(feature = "nod"))]
mod tool {
    use super::RvzCompressionAlgorithm;
    use crate::progress::get_none_progress_style;
    use crate::util::{get_executable_path, run_tool};
    use anyhow::{Context, Result};
    use indicatif::ProgressBar;
    use std::path::Path;
    use std::time::Duration;
    use tokio::process::Command;

    pub const DOLPHIN_TOOL_EXECUTABLES: &[&str] = &["dolphin-tool", "DolphinTool"];

    pub const BACKEND_NAME: &str = "dolphin-tool";

    pub const SUPPORTS_SCRUB: bool = true;

    /// A subprocess reports nothing usable, so all it gets is a spinner.
    fn spin(progress_bar: &ProgressBar) {
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));
    }

    pub async fn to_iso<P: AsRef<Path>, Q: AsRef<Path>>(
        source: P,
        destination: Q,
        progress_bar: &ProgressBar,
    ) -> Result<()> {
        spin(progress_bar);
        run_tool(
            Command::new(get_executable_path(DOLPHIN_TOOL_EXECUTABLES)?)
                .arg("convert")
                .arg("-f")
                .arg("iso")
                .arg("-i")
                .arg(source.as_ref())
                .arg("-o")
                .arg(destination.as_ref()),
        )
        .await?;
        Ok(())
    }

    pub async fn to_rvz<P: AsRef<Path>, Q: AsRef<Path>>(
        source: P,
        destination: Q,
        progress_bar: &ProgressBar,
        compression_algorithm: &RvzCompressionAlgorithm,
        compression_level: usize,
        block_size: usize,
        scrub: bool,
    ) -> Result<()> {
        spin(progress_bar);
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
            .arg(source.as_ref())
            .arg("-o")
            .arg(destination.as_ref());
        if scrub {
            command.arg("-s");
        }
        run_tool(&mut command).await?;
        Ok(())
    }

    pub async fn get_version() -> Result<String> {
        let output = Command::new(get_executable_path(DOLPHIN_TOOL_EXECUTABLES)?)
            .output()
            .await
            .context("Failed to spawn dolphin-tool")?;
        // dolphin-tool doesn't advertize any version
        String::from_utf8(output.stderr).unwrap();
        let version = String::from("unknown");
        Ok(version)
    }
}

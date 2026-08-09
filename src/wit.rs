use super::common::*;
use super::mimetype::*;
use anyhow::Result;
use indicatif::ProgressBar;
use std::path::Path;

// Whichever backend handles WBFS in this build. Both expose the same items, so
// nothing below this line names either of them.
#[cfg(not(feature = "nod"))]
use self::tool as backend;
#[cfg(feature = "nod")]
use super::nod as backend;

pub use backend::BACKEND_NAME;

pub struct WbfsRomfile {
    // kept for consistency with the other format wrappers, not read back after conversion
    #[allow(dead_code)]
    romfile: CommonRomfile,
}

pub trait ToWbfs {
    async fn to_wbfs<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<WbfsRomfile>;
}

impl ToWbfs for IsoRomfile {
    async fn to_wbfs<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<WbfsRomfile> {
        progress_bar.set_message("Creating wbfs");

        let path = destination_directory
            .as_ref()
            .join(self.romfile.path.file_name().unwrap())
            .with_extension(WBFS_EXTENSION);

        backend::to_wbfs(&self.romfile.path, &path, progress_bar).await?;

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        Ok(WbfsRomfile {
            romfile: CommonRomfile::from_path(&path)?,
        })
    }
}

pub async fn get_version() -> Result<String> {
    backend::get_version().await
}

/// The wit backend: WBFS by way of the external executable.
#[cfg(not(feature = "nod"))]
mod tool {
    use crate::progress::get_none_progress_style;
    use crate::util::run_tool;
    use anyhow::{Context, Result};
    use indicatif::ProgressBar;
    use regex::Regex;
    use std::path::Path;
    use std::sync::LazyLock;
    use std::time::Duration;
    use tokio::process::Command;

    const WIT: &str = "wit";

    static VERSION_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+\.[\d\w]+").unwrap());

    pub const BACKEND_NAME: &str = "wit";

    pub async fn to_wbfs<P: AsRef<Path>, Q: AsRef<Path>>(
        source: P,
        destination: Q,
        progress_bar: &ProgressBar,
    ) -> Result<()> {
        // A subprocess reports nothing usable, so all it gets is a spinner
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));
        run_tool(
            Command::new(WIT)
                .arg("COPY")
                .arg("--wbfs")
                .arg("--source")
                .arg(source.as_ref())
                .arg("--dest")
                .arg(destination.as_ref()),
        )
        .await?;
        Ok(())
    }

    pub async fn get_version() -> Result<String> {
        let output = Command::new(WIT)
            .arg("--version")
            .output()
            .await
            .context("Failed to spawn wit")?;

        let stdout = String::from_utf8(output.stdout).unwrap();
        let version = stdout
            .lines()
            .next()
            .and_then(|line| VERSION_REGEX.find(line))
            .map(|version| version.as_str().to_string())
            .unwrap_or(String::from("unknown"));

        Ok(version)
    }
}

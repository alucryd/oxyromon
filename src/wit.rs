use super::common::*;
use super::mimetype::*;
use super::progress::stop_action;
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

        stop_action(progress_bar);

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
    use crate::progress::start_action;
    use crate::util::{run_tool, tool_version};
    use anyhow::Result;
    use indicatif::ProgressBar;
    use regex::Regex;
    use std::path::Path;
    use std::sync::LazyLock;
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
        start_action(progress_bar, None);
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
        tool_version(WIT, "wit", &["--version"], true, 0, Some(&VERSION_REGEX)).await
    }
}

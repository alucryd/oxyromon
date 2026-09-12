use super::common::*;
use super::mimetype::*;
use super::progress::*;
use super::util::*;
use anyhow::{Result, bail};
use regex::Regex;
use std::sync::LazyLock;
use tokio::process::Command;

const XDELTA3: &str = "xdelta3";

static VERSION_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+\.\d+\.\d+").unwrap());

// patch application is not wired up yet, kept for the planned feature
#[allow(dead_code)]
pub struct XdeltaRomfile {
    pub romfile: CommonRomfile,
}

impl Patch for XdeltaRomfile {
    async fn patch<P: AsRef<std::path::Path>>(
        &self,
        progress_bar: &indicatif::ProgressBar,
        romfile: &CommonRomfile,
        destination_directory: &P,
    ) -> Result<CommonRomfile> {
        start_action(
            progress_bar,
            Some(&format!(
                "Applying \"{}\"",
                self.romfile.path.file_name().unwrap().to_str().unwrap()
            )),
        );

        print_action(
            progress_bar,
            &format!(
                "Patching \"{}\"",
                romfile.path.file_name().unwrap().to_str().unwrap()
            ),
        );

        let path = destination_directory
            .as_ref()
            .join(romfile.path.file_name().unwrap());

        let output = Command::new(XDELTA3)
            .arg("-d")
            .arg("-s")
            .arg(&romfile.path)
            .arg(&path)
            .arg(&self.romfile.path)
            .output()
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "Failed to patch \"{}\"",
                    romfile.path.file_name().unwrap().to_str().unwrap()
                )
            });

        if !output.status.success() {
            bail!("{}", String::from_utf8_lossy(&output.stderr))
        }

        stop_action(progress_bar);

        CommonRomfile::from_path(&path)
    }
}

#[allow(dead_code)]
pub trait AsXdelta {
    fn as_xdelta(self) -> Result<XdeltaRomfile>;
}

impl AsXdelta for CommonRomfile {
    fn as_xdelta(self) -> Result<XdeltaRomfile> {
        if self
            .path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase()
            != XDELTA_EXTENSION
        {
            bail!("Not a valid xdelta");
        }
        Ok(XdeltaRomfile { romfile: self })
    }
}

pub async fn get_version() -> Result<String> {
    tool_version(XDELTA3, "xdelta3", &["-V"], false, 0, Some(&VERSION_REGEX)).await
}

use super::common::*;
use super::progress::*;
use super::util::*;
use anyhow::{Context, Result, bail};
use indicatif::ProgressBar;
use std::path::Path;
use std::str::FromStr;
use strum::{Display, EnumString};
use tokio::process::Command;

const FLIPS: &str = "flips";

// patch application is not wired up yet, kept for the planned feature
#[allow(dead_code)]
#[derive(Clone, Copy, Display, EnumString, PartialEq, Eq)]
#[strum(serialize_all = "lowercase")]
pub enum XpsType {
    Bps,
    Ips,
}

#[allow(dead_code)]
pub struct XpsRomfile {
    pub romfile: CommonRomfile,
    pub xps_type: XpsType,
}

impl Patch for XpsRomfile {
    async fn patch<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
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

        let output = Command::new(FLIPS)
            .arg("--apply")
            .arg(&self.romfile.path)
            .arg(&romfile.path)
            .arg(&path)
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
pub trait AsXps {
    fn as_xps(self) -> Result<XpsRomfile>;
}

impl AsXps for CommonRomfile {
    fn as_xps(self) -> Result<XpsRomfile> {
        let xps_type = XpsType::from_str(
            &self
                .path
                .extension()
                .unwrap()
                .to_str()
                .unwrap()
                .to_lowercase(),
        )
        .context("Not a valid xps")?;
        Ok(XpsRomfile {
            romfile: self,
            xps_type,
        })
    }
}

pub async fn get_version() -> Result<String> {
    tool_version(FLIPS, "flips", &["-v"], false, 0, None).await
}

#[cfg(test)]
mod test_patch;

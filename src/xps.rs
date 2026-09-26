use super::common::*;
use super::progress::*;
use anyhow::{Context, Result};
use indicatif::ProgressBar;
use std::path::Path;
use std::str::FromStr;
use strum::{Display, EnumString};

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

        let (source, patch, output) = (
            romfile.path.clone(),
            self.romfile.path.clone(),
            path.clone(),
        );
        let warning = run_blocking(progress_bar, patch.metadata()?.len(), move |progress| {
            xps_rs::apply(&source, &patch, &output, progress)
        })
        .await?;
        if let Some(warning) = warning {
            print_warning(progress_bar, &format!("Patched, but {warning}"));
        }

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

/// Reported by `info`.
pub async fn get_version() -> Result<String> {
    Ok(String::from("built-in"))
}

#[cfg(test)]
mod test_patch;

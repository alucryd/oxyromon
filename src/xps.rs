use super::common::*;
use super::progress::*;
use anyhow::{Result, bail};
use indicatif::ProgressBar;
use std::path::Path;

/// An IPS or BPS patch; which one is told by xps-rs from its magic, when it
/// applies it.
pub struct XpsRomfile {
    pub romfile: CommonRomfile,
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
        let extension = self
            .path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_lowercase();
        if !matches!(extension.as_str(), "bps" | "ips") {
            bail!("Not a valid xps");
        }
        Ok(XpsRomfile { romfile: self })
    }
}

/// Reported by `info`.
pub async fn get_version() -> Result<String> {
    Ok(String::from("built-in"))
}

#[cfg(test)]
mod test_patch;

use super::common::*;
use super::mimetype::*;
use super::progress::*;
use anyhow::{Result, bail};
use indicatif::ProgressBar;
use std::path::Path;

// patch application is not wired up yet, kept for the planned feature
#[allow(dead_code)]
pub struct XdeltaRomfile {
    pub romfile: CommonRomfile,
}

impl Patch for XdeltaRomfile {
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
        run_blocking(progress_bar, patch.metadata()?.len(), move |progress| {
            xdelta_rs::decode(Some(&source), &patch, &output, progress)
        })
        .await?;

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

/// Reported by `info`.
pub async fn get_version() -> Result<String> {
    Ok(String::from("built-in"))
}

#[cfg(test)]
mod test_patch;

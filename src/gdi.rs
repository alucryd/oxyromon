//! GDI, what Dreamcast optical drive emulators such as GDEMU load, made from
//! Redump's CUE/BIN by the gdi-rs crate, a port of gdidrop.

use super::common::*;
use super::mimetype::*;
use super::progress::*;
use anyhow::{Context, Result, bail};
use indicatif::ProgressBar;
use std::path::Path;

#[derive(Clone)]
pub struct GdiRomfile {
    pub gdi_romfile: CommonRomfile,
    pub track_romfiles: Vec<CommonRomfile>,
}

pub trait AsGdi {
    fn as_gdi(self, track_romfiles: Vec<CommonRomfile>) -> Result<GdiRomfile>;
}

pub trait ToGdi {
    async fn to_gdi<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<GdiRomfile>;
}

// === GDI TRAIT IMPLEMENTATIONS ===

impl AsGdi for CommonRomfile {
    fn as_gdi(self, track_romfiles: Vec<CommonRomfile>) -> Result<GdiRomfile> {
        if self
            .path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase()
            != GDI_EXTENSION
        {
            bail!("Not a valid gdi");
        }
        // Validate track files (can be .bin, .raw, or other valid extensions)
        for track_romfile in &track_romfiles {
            let extension = track_romfile
                .path
                .extension()
                .unwrap()
                .to_str()
                .unwrap()
                .to_lowercase();
            if extension != BIN_EXTENSION && extension != RAW_EXTENSION {
                bail!("Not a valid track file extension: {}", extension);
            }
        }
        Ok(GdiRomfile {
            gdi_romfile: self,
            track_romfiles,
        })
    }
}

impl ToGdi for CueBinRomfile {
    async fn to_gdi<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<GdiRomfile> {
        start_action(progress_bar, Some("Converting CUE/BIN to GDI"));

        let cue = self.cue_romfile.path.clone();
        let destination = destination_directory.as_ref().to_path_buf();
        // The CUE's BINs, which the conversion reports reading.
        let length = gdi_rs::input_size(&cue)?;
        let gdi = run_blocking(progress_bar, length, move |progress| {
            gdi_rs::convert(&cue, &destination, progress)
        })
        .await
        .with_context(|| {
            format!(
                "Failed to convert \"{}\" to GDI",
                self.cue_romfile.path.display()
            )
        })?;

        stop_action(progress_bar);

        Ok(GdiRomfile {
            gdi_romfile: CommonRomfile::from_path(&gdi.gdi)?,
            track_romfiles: gdi
                .tracks
                .iter()
                .map(CommonRomfile::from_path)
                .collect::<Result<Vec<CommonRomfile>>>()?,
        })
    }
}

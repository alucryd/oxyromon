use super::common::*;
use super::mimetype::*;
use super::progress::stop_action;
use anyhow::Result;
use indicatif::ProgressBar;
use std::path::Path;

use super::rvz::write_disc;
use nod::common::Format;
use nod::write::FormatOptions;

pub trait ToWbfs {
    async fn to_wbfs<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<()>;
}

impl ToWbfs for IsoRomfile {
    async fn to_wbfs<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<()> {
        progress_bar.set_message("Creating wbfs");

        let path = destination_directory
            .as_ref()
            .join(self.romfile.path.file_name().unwrap())
            .with_extension(WBFS_EXTENSION);

        write_disc(
            &self.romfile.path,
            &path,
            progress_bar,
            FormatOptions::new(Format::Wbfs),
        )
        .await?;

        stop_action(progress_bar);

        Ok(())
    }
}

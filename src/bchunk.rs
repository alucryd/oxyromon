use super::common::*;
use super::mimetype::*;
use super::progress::*;
use super::util::*;
use anyhow::{Result, bail};
use indicatif::ProgressBar;
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;
use tokio::process::Command;

const BCHUNK: &str = "bchunk";

static VERSION_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+\.\d+\.\d+").unwrap());

impl ToIso for CueBinRomfile {
    async fn to_iso<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<IsoRomfile> {
        if self.bin_romfiles.len() > 1 {
            bail!("Only single bins are supported");
        }

        start_action(progress_bar, Some("Creating iso"));

        let path = destination_directory
            .as_ref()
            .join(self.cue_romfile.path.file_name().unwrap())
            .with_extension(ISO_EXTENSION);

        run_tool(
            Command::new(BCHUNK)
                .arg(&self.bin_romfiles.first().unwrap().path)
                .arg(&self.cue_romfile.path)
                .arg(BCHUNK)
                .current_dir(destination_directory.as_ref()),
        )
        .await?;

        rename_file(
            progress_bar,
            &destination_directory
                .as_ref()
                .join(format!("{}01.iso", BCHUNK)),
            &path,
            true,
        )
        .await?;

        stop_action(progress_bar);

        CommonRomfile::from_path(&path)?.as_iso()
    }
}

pub async fn get_version() -> Result<String> {
    tool_version(BCHUNK, "bchunk", &[], true, 0, Some(&VERSION_REGEX)).await
}

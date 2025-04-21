use super::SimpleResult;
use super::common::*;
use super::mimetype::*;
use super::progress::*;
use super::util::*;
use indicatif::ProgressBar;
use lazy_static::lazy_static;
use regex::Regex;
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;

const BCHUNK: &str = "bchunk";

lazy_static! {
    static ref VERSION_REGEX: Regex = Regex::new(r"\d+\.\d+\.\d+").unwrap();
}

impl ToIso for CueBinRomfile {
    async fn to_iso<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> simple_error::SimpleResult<IsoRomfile> {
        if self.bin_romfiles.len() > 1 {
            bail!("Only single bins are supported");
        }

        progress_bar.set_message("Creating iso");
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        let path = destination_directory
            .as_ref()
            .join(self.cue_romfile.path.file_name().unwrap())
            .with_extension(ISO_EXTENSION);

        let output = Command::new(BCHUNK)
            .arg(&self.bin_romfiles.first().unwrap().path)
            .arg(&self.cue_romfile.path)
            .arg(BCHUNK)
            .current_dir(destination_directory.as_ref())
            .output()
            .await
            .expect("Failed to create iso");

        if !output.status.success() {
            bail!(String::from_utf8(output.stderr).unwrap().as_str())
        }

        rename_file(
            progress_bar,
            &destination_directory
                .as_ref()
                .join(format!("{}01.iso", BCHUNK)),
            &path,
            true,
        )
        .await?;

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        CommonRomfile::from_path(&path)?.as_iso()
    }
}

pub async fn get_version() -> SimpleResult<String> {
    let output = try_with!(
        Command::new(BCHUNK).output().await,
        "Failed to spawn bchunk"
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let version = stdout
        .lines()
        .next()
        .and_then(|line| VERSION_REGEX.find(line))
        .map(|version| version.as_str().to_string())
        .unwrap_or(String::from("unknown"));

    Ok(version)
}

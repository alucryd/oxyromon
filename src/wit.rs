use super::SimpleResult;
use super::common::*;
use super::config::*;
use super::progress::*;
use indicatif::ProgressBar;
use lazy_static::lazy_static;
use regex::Regex;
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;

const WIT: &str = "wit";

lazy_static! {
    static ref VERSION_REGEX: Regex = Regex::new(r"\d+\.[\d\w]+").unwrap();
}

pub struct WbfsRomfile {
    romfile: CommonRomfile,
}

pub trait ToWbfs {
    async fn to_wbfs<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> SimpleResult<WbfsRomfile>;
}

impl ToWbfs for IsoRomfile {
    async fn to_wbfs<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> simple_error::SimpleResult<WbfsRomfile> {
        progress_bar.set_message("Creating wbfs");
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        let path = destination_directory
            .as_ref()
            .join(self.romfile.path.file_name().unwrap())
            .with_extension(WBFS_EXTENSION);

        let output = Command::new(WIT)
            .arg("COPY")
            .arg("--wbfs")
            .arg("--source")
            .arg(&self.romfile.path)
            .arg("--dest")
            .arg(&path)
            .output()
            .await
            .expect("Failed to create wbfs");

        if !output.status.success() {
            bail!(String::from_utf8(output.stderr).unwrap().as_str())
        }

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        Ok(WbfsRomfile {
            romfile: CommonRomfile::from_path(&path)?,
        })
    }
}

pub async fn get_version() -> SimpleResult<String> {
    let output = try_with!(
        Command::new(WIT).arg("--version").output().await,
        "Failed to spawn wit"
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

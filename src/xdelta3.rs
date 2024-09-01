use super::common::*;
use super::config::*;
use super::model::*;
use super::progress::*;
use super::SimpleResult;
use lazy_static::lazy_static;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

const XDELTA3: &str = "xdelta3";

lazy_static! {
    static ref VERSION_REGEX: Regex = Regex::new(r"\d+\.\d+\.\d+").unwrap();
}

pub struct XdeltaRomfile {
    pub path: PathBuf,
}

impl AsCommon for XdeltaRomfile {
    fn as_common(&self) -> SimpleResult<CommonRomfile> {
        CommonRomfile::from_path(&self.path)
    }
}

impl PatchFile for XdeltaRomfile {
    async fn patch<P: AsRef<std::path::Path>>(
        &self,
        progress_bar: &indicatif::ProgressBar,
        romfile: &CommonRomfile,
        destination_directory: &P,
    ) -> simple_error::SimpleResult<CommonRomfile> {
        progress_bar.set_message(format!("Applying \"{}\"", &self.as_common()?));
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

        progress_bar.println(format!(
            "Patching \"{}\"",
            &romfile.path.file_name().unwrap().to_str().unwrap()
        ));

        let path = destination_directory
            .as_ref()
            .join(romfile.path.file_name().unwrap());

        let output = Command::new(XDELTA3)
            .arg("-d")
            .arg("-s")
            .arg(&romfile.path)
            .arg(&path)
            .arg(&self.path)
            .output()
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "Failed to patch \"{}\"",
                    &romfile.path.file_name().unwrap().to_str().unwrap()
                )
            });

        if !output.status.success() {
            bail!(String::from_utf8(output.stderr).unwrap().as_str())
        }

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

        Ok(CommonRomfile { path })
    }
}

impl FromPath<XdeltaRomfile> for XdeltaRomfile {
    fn from_path<P: AsRef<Path>>(path: &P) -> SimpleResult<XdeltaRomfile> {
        let path = path.as_ref().to_path_buf();
        let extension = path.extension().unwrap().to_str().unwrap().to_lowercase();
        if extension != XDELTA_EXTENSION {
            bail!("Not a valid xdelta");
        }
        Ok(XdeltaRomfile { path })
    }
}

pub trait AsXdelta {
    fn as_xdelta(&self) -> SimpleResult<XdeltaRomfile>;
}

impl AsXdelta for Romfile {
    fn as_xdelta(&self) -> SimpleResult<XdeltaRomfile> {
        XdeltaRomfile::from_path(&self.path)
    }
}

pub async fn get_version() -> SimpleResult<String> {
    let output = try_with!(
        Command::new(XDELTA3).arg("-V").output().await,
        "Failed to spawn xdelta3"
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    let version = stderr
        .lines()
        .next()
        .and_then(|line| VERSION_REGEX.find(line))
        .map(|version| version.as_str().to_string())
        .unwrap_or(String::from("unknown"));

    Ok(version)
}

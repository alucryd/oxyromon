use super::SimpleResult;
use super::common::*;
use super::mimetype::*;
use super::progress::*;
use lazy_static::lazy_static;
use regex::Regex;
use std::time::Duration;
use tokio::process::Command;

const XDELTA3: &str = "xdelta3";

lazy_static! {
    static ref VERSION_REGEX: Regex = Regex::new(r"\d+\.\d+\.\d+").unwrap();
}

pub struct XdeltaRomfile {
    pub romfile: CommonRomfile,
}

impl PatchFile for XdeltaRomfile {
    async fn patch<P: AsRef<std::path::Path>>(
        &self,
        progress_bar: &indicatif::ProgressBar,
        romfile: &CommonRomfile,
        destination_directory: &P,
    ) -> simple_error::SimpleResult<CommonRomfile> {
        progress_bar.set_message(format!(
            "Applying \"{}\"",
            &self.romfile.path.file_name().unwrap().to_str().unwrap()
        ));
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
            .arg(&self.romfile.path)
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

        CommonRomfile::from_path(&path)
    }
}

pub trait AsXdelta {
    fn as_xdelta(self) -> SimpleResult<XdeltaRomfile>;
}

impl AsXdelta for CommonRomfile {
    fn as_xdelta(self) -> SimpleResult<XdeltaRomfile> {
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

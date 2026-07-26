use super::common::*;
use super::mimetype::*;
use super::progress::*;
use anyhow::{Context, Result, bail};
use regex::Regex;
use std::sync::LazyLock;
use std::time::Duration;
use tokio::process::Command;

const XDELTA3: &str = "xdelta3";

static VERSION_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+\.\d+\.\d+").unwrap());

// patch application is not wired up yet, kept for the planned feature
#[allow(dead_code)]
pub struct XdeltaRomfile {
    pub romfile: CommonRomfile,
}

impl Patch for XdeltaRomfile {
    async fn patch<P: AsRef<std::path::Path>>(
        &self,
        progress_bar: &indicatif::ProgressBar,
        romfile: &CommonRomfile,
        destination_directory: &P,
    ) -> Result<CommonRomfile> {
        progress_bar.set_message(format!(
            "Applying \"{}\"",
            self.romfile.path.file_name().unwrap().to_str().unwrap()
        ));
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));

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
                    romfile.path.file_name().unwrap().to_str().unwrap()
                )
            });

        if !output.status.success() {
            bail!("{}", String::from_utf8_lossy(&output.stderr))
        }

        progress_bar.set_message("");
        progress_bar.disable_steady_tick();

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

pub async fn get_version() -> Result<String> {
    let output = Command::new(XDELTA3)
        .arg("-V")
        .output()
        .await
        .context("Failed to spawn xdelta3")?;

    let stderr = String::from_utf8(output.stderr).unwrap();
    let version = stderr
        .lines()
        .next()
        .and_then(|line| VERSION_REGEX.find(line))
        .map(|version| version.as_str().to_string())
        .unwrap_or(String::from("unknown"));

    Ok(version)
}

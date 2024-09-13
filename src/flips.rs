use super::common::*;
use super::model::*;
use super::progress::*;
use super::SimpleResult;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;
use strum::{Display, EnumString};
use tokio::process::Command;

const FLIPS: &str = "flips";

#[derive(Clone, Copy, Display, EnumString, PartialEq, Eq)]
#[strum(serialize_all = "lowercase")]
pub enum XpsType {
    Bps,
    Ips,
}

pub struct XpsRomfile {
    pub path: PathBuf,
    pub xps_type: XpsType,
}

impl AsCommon for XpsRomfile {
    fn as_common(&self) -> SimpleResult<CommonRomfile> {
        CommonRomfile::from_path(&self.path)
    }
}

impl PatchFile for XpsRomfile {
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

        let output = Command::new(FLIPS)
            .arg("--apply")
            .arg(&self.path)
            .arg(&romfile.path)
            .arg(&path)
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

impl FromPath<XpsRomfile> for XpsRomfile {
    fn from_path<P: AsRef<Path>>(path: &P) -> SimpleResult<XpsRomfile> {
        let path = path.as_ref().to_path_buf();
        let extension = path.extension().unwrap().to_str().unwrap().to_lowercase();
        let xps_type = try_with!(XpsType::from_str(&extension), "Not a valid xps");
        Ok(XpsRomfile { path, xps_type })
    }
}

pub trait AsXps {
    fn as_xps(&self) -> SimpleResult<XpsRomfile>;
}

impl AsXps for Romfile {
    fn as_xps(&self) -> SimpleResult<XpsRomfile> {
        XpsRomfile::from_path(&self.path)
    }
}

pub async fn get_version() -> SimpleResult<String> {
    let output = try_with!(
        Command::new(FLIPS).arg("-v").output().await,
        "Failed to spawn flips"
    );

    // flips doesn't advertise any version
    String::from_utf8(output.stderr).unwrap();
    let version = String::from("unknown");

    Ok(version)
}

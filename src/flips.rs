use super::SimpleResult;
use super::common::*;
use super::progress::*;
use indicatif::ProgressBar;
use std::path::Path;
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
    pub romfile: CommonRomfile,
    pub xps_type: XpsType,
}

impl PatchFile for XpsRomfile {
    async fn patch<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
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

        let output = Command::new(FLIPS)
            .arg("--apply")
            .arg(&self.romfile.path)
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

        CommonRomfile::from_path(&path)
    }
}

pub trait AsXps {
    fn as_xps(self) -> SimpleResult<XpsRomfile>;
}

impl AsXps for CommonRomfile {
    fn as_xps(self) -> SimpleResult<XpsRomfile> {
        let xps_type = try_with!(
            XpsType::from_str(
                &self
                    .path
                    .extension()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_lowercase()
            ),
            "Not a valid xps"
        );
        Ok(XpsRomfile {
            romfile: self,
            xps_type,
        })
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

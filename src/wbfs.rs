//! WBFS, the Wii backup loaders' disc image, by way of wbfs-rs, which writes
//! it as wit does: scrubbed, so export only.
//!
//! From an ISO, a Wii disc needs the Wii common key to find what it uses,
//! which `import-keys` stores. From an RVZ, which keeps its title keys, it
//! needs none, and no ISO is decoded on the way.

use super::common::*;
use super::mimetype::*;
use super::progress::*;
use super::rvz::{RvzRomfile, load_wii_keys};
use anyhow::{Context, Result};
use indicatif::ProgressBar;
use std::path::Path;

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
        write_wbfs(
            &self.romfile.path,
            destination_directory.as_ref(),
            progress_bar,
            load_wii_keys()?,
        )
        .await
    }
}

impl ToWbfs for RvzRomfile {
    async fn to_wbfs<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<()> {
        write_wbfs(
            &self.romfile.path,
            destination_directory.as_ref(),
            progress_bar,
            Vec::new(),
        )
        .await
    }
}

/// Write the disc at `source`, an ISO or an RVZ, to a WBFS named after it.
async fn write_wbfs(
    source: &Path,
    destination_directory: &Path,
    progress_bar: &ProgressBar,
    keys: Vec<wbfs_rs::Key>,
) -> Result<()> {
    progress_bar.set_message("Creating wbfs");

    let path = destination_directory
        .join(source.file_name().unwrap())
        .with_extension(WBFS_EXTENSION);

    print_action(
        progress_bar,
        &format!(
            "Creating \"{}\"",
            path.file_name().unwrap().to_str().unwrap()
        ),
    );

    let (input, output) = (source.to_path_buf(), path.clone());
    run_blocking(progress_bar, input.metadata()?.len(), move |progress| {
        wbfs_rs::create(&input, &output, &keys, progress)
    })
    .await
    .map_err(|error| match error.downcast_ref() {
        Some(wbfs_rs::Error::MissingKey(_)) => {
            error.context("A Wii disc needs the Wii common key: run import-keys")
        }
        _ => error,
    })
    .with_context(|| format!("Failed to create \"{}\"", path.display()))?;

    stop_action(progress_bar);

    Ok(())
}

/// Reported by `info`.
pub async fn get_version() -> Result<String> {
    Ok(String::from("built-in"))
}

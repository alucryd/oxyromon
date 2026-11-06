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
            Some(load_wii_keys()?),
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
            None,
        )
        .await
    }
}

/// An RVZ as a disc wbfs-rs reads, through rvz-rs: the title keys it keeps
/// stand in for the common key.
struct RvzSource(rvz_rs::Reader);

impl wbfs_rs::Source for RvzSource {
    fn size(&self) -> u64 {
        self.0.size()
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> wbfs_rs::Result<()> {
        self.0
            .read_at(offset, buf)
            .map_err(|error| wbfs_rs::Error::Io(std::io::Error::other(error)))
    }

    fn title_key(&self, data_offset: u64) -> Option<wbfs_rs::Key> {
        self.0
            .title_keys()
            .find(|(offset, _)| *offset == data_offset)
            .map(|(_, key)| key)
    }
}

/// Write the disc at `source` to a WBFS named after it: an ISO, with `keys`,
/// or an RVZ, with none.
async fn write_wbfs(
    source: &Path,
    destination_directory: &Path,
    progress_bar: &ProgressBar,
    keys: Option<Vec<wbfs_rs::Key>>,
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
    let input_size = input.metadata()?.len();
    run_blocking(progress_bar, input_size, move |progress| {
        let Some(keys) = keys else {
            let reader = rvz_rs::Reader::open(&input)
                .map_err(|error| wbfs_rs::Error::Io(std::io::Error::other(error)))?;
            // wbfs-rs counts the disc's bytes, the bar the RVZ's
            let disc_size = reader.size().max(1);
            let (mut consumed, mut reported) = (0u64, 0u64);
            let mut scaled = |n: u64| {
                consumed += n;
                let position = (consumed as u128 * input_size as u128 / disc_size as u128) as u64;
                progress(position - reported);
                reported = position;
            };
            return wbfs_rs::create_from(&mut RvzSource(reader), &output, &[], &mut scaled);
        };
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

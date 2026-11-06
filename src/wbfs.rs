//! WBFS, the Wii backup loaders' disc image, by way of nod.

use super::common::*;
use super::mimetype::*;
use super::progress::*;
use anyhow::{Context, Result};
use indicatif::ProgressBar;
use nod::common::Format;
use nod::read::{DiscOptions, DiscReader, PartitionEncryption};
use nod::write::{DiscWriter, DiscWriterWeight, FormatOptions, ProcessOptions, ScrubLevel};
use std::fs::File;
use std::io::{Seek, Write};
use std::path::Path;
use tokio::task::spawn_blocking;

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
        progress_bar.set_message("Creating wbfs");

        let path = destination_directory
            .as_ref()
            .join(self.romfile.path.file_name().unwrap())
            .with_extension(WBFS_EXTENSION);

        write_wbfs(&self.romfile.path, &path, progress_bar).await?;

        stop_action(progress_bar);

        Ok(())
    }
}

/// How much of the machine to give the writer, following nod's own guidance
/// that a heavier writer earns more threads.
fn processor_threads(weight: DiscWriterWeight) -> usize {
    let available = std::thread::available_parallelism()
        .map(|available| available.get())
        .unwrap_or(1);
    match weight {
        DiscWriterWeight::Light => 0,
        DiscWriterWeight::Medium => available.min(4),
        DiscWriterWeight::Heavy => available,
    }
}

/// Convert an ISO to WBFS, leaving partition hashes and encryption as they are
/// on the source.
async fn write_wbfs(source: &Path, destination: &Path, progress_bar: &ProgressBar) -> Result<()> {
    let source = source.to_path_buf();
    let destination = destination.to_path_buf();
    let progress_bar = progress_bar.clone();
    spawn_blocking(move || {
        let disc = DiscReader::new(
            &source,
            &DiscOptions {
                partition_encryption: PartitionEncryption::Original,
                preloader_threads: 4,
            },
        )
        .with_context(|| format!("Failed to open \"{}\"", source.display()))?;
        let writer = DiscWriter::new(disc, &FormatOptions::new(Format::Wbfs))
            .context("Failed to set up the WBFS writer")?;
        let mut file = File::create(&destination)
            .with_context(|| format!("Failed to create \"{}\"", destination.display()))?;
        let process_options = ProcessOptions {
            processor_threads: processor_threads(writer.weight()),
            digest_crc32: false,
            digest_md5: false,
            digest_sha1: false,
            digest_xxh64: false,
            scrub: ScrubLevel::None,
        };
        progress_bar.reset();
        progress_bar.set_style(get_bytes_progress_style());
        progress_bar.set_length(writer.progress_bound());
        let finalization = writer
            .process(
                |data, progress, _| {
                    file.write_all(data.as_ref())?;
                    progress_bar.set_position(progress);
                    Ok(())
                },
                &process_options,
            )
            .with_context(|| format!("Failed to write \"{}\"", destination.display()))?;
        // WBFS only knows its header once every block has been written
        if !finalization.header.is_empty() {
            file.rewind()?;
            file.write_all(finalization.header.as_ref())?;
        }
        file.flush()?;
        Ok(())
    })
    .await
    .context("Disc writer task failed")?
}

/// Reported by `info`.
pub async fn get_version() -> Result<String> {
    Ok(String::from("built-in"))
}

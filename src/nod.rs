//! GameCube and Wii disc image conversion backed by the [`nod`] crate.
//!
//! Compiled in by the `nod` feature, where it stands in for the `dolphin-tool`
//! and `wit` subprocesses. Everything nod does is synchronous and CPU bound, so
//! each entry point hands its work to the blocking pool rather than occupying a
//! runtime worker for the length of a conversion.

use super::common::hash_reader;
use super::config::HashAlgorithm;
use super::dolphin::RvzCompressionAlgorithm;
use super::progress::get_bytes_progress_style;
use anyhow::{Context, Result};
use indicatif::ProgressBar;
use nod::common::{Compression, Format};
use nod::read::{DiscOptions, DiscReader, PartitionEncryption};
use nod::write::{DiscWriter, DiscWriterWeight, FormatOptions, ProcessOptions, ScrubLevel};
use std::fs::File;
use std::io::{Seek, Write};
use std::path::Path;
use tokio::task::spawn_blocking;

/// Threads nod uses to read ahead while decoding a compressed image.
const PRELOADER_THREADS: usize = 4;

/// Open a disc image, reproducing it exactly: partition hashes and encryption
/// are left as they are on the source, so an extracted ISO is byte for byte
/// what went in.
fn open(path: &Path) -> Result<DiscReader> {
    DiscReader::new(
        path,
        &DiscOptions {
            partition_encryption: PartitionEncryption::Original,
            preloader_threads: PRELOADER_THREADS,
        },
    )
    .with_context(|| format!("Failed to open \"{}\"", path.display()))
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

/// Map oxyromon's configured algorithm and level onto nod's [`Compression`].
///
/// `RVZ_COMPRESSION_LEVEL` ranges over 1..=22, which only Zstandard accepts in
/// full: the others cap at 9. The level is clamped rather than rejected, so an
/// existing configuration keeps working instead of failing the conversion.
fn compression(algorithm: &RvzCompressionAlgorithm, level: usize) -> Compression {
    match algorithm {
        RvzCompressionAlgorithm::None => Compression::None,
        RvzCompressionAlgorithm::Bzip2 => Compression::Bzip2(level.clamp(1, 9) as u8),
        RvzCompressionAlgorithm::Lzma => Compression::Lzma(level.clamp(1, 9) as u8),
        RvzCompressionAlgorithm::Lzma2 => Compression::Lzma2(level.clamp(1, 9) as u8),
        RvzCompressionAlgorithm::Zstd => Compression::Zstandard(level.clamp(1, 22) as i8),
    }
}

/// Extract any nod-supported disc image to a raw ISO.
pub async fn to_iso<P: AsRef<Path>, Q: AsRef<Path>>(
    source: P,
    destination: Q,
    progress_bar: &ProgressBar,
) -> Result<()> {
    let source = source.as_ref().to_path_buf();
    let destination = destination.as_ref().to_path_buf();
    let progress_bar = progress_bar.clone();
    spawn_blocking(move || {
        let mut disc = open(&source)?;
        let mut file = File::create(&destination)
            .with_context(|| format!("Failed to create \"{}\"", destination.display()))?;
        // Unlike a subprocess, the decoder can say how far along it is
        progress_bar.reset();
        progress_bar.set_style(get_bytes_progress_style());
        progress_bar.set_length(disc.disc_size());
        nod::util::buf_copy(&mut disc, &mut progress_bar.wrap_write(&mut file))
            .with_context(|| format!("Failed to write \"{}\"", destination.display()))?;
        file.flush()?;
        Ok(())
    })
    .await
    .context("Disc reader task failed")?
}

/// Hash a disc image's contents without writing the decoded ISO anywhere.
///
/// This is what the extract-then-hash path costs on a dual layer Wii disc:
/// several gigabytes written to the temp directory and read straight back. The
/// decoder is just a reader, so the digest can consume it directly.
pub async fn hash<P: AsRef<Path>>(
    source: P,
    progress_bar: &ProgressBar,
    hash_algorithm: &HashAlgorithm,
) -> Result<(String, u64)> {
    let source = source.as_ref().to_path_buf();
    let hash_algorithm = *hash_algorithm;
    let progress_bar = progress_bar.clone();
    spawn_blocking(move || {
        let mut disc = open(&source)?;
        progress_bar.set_style(get_bytes_progress_style());
        progress_bar.set_length(disc.disc_size());
        hash_reader(&mut disc, &progress_bar, &hash_algorithm)
    })
    .await
    .context("Disc reader task failed")?
}

/// Convert an ISO to RVZ.
///
/// `scrub` is accepted to match the dolphin-tool backend's signature but is
/// ignored: see [`SUPPORTS_SCRUB`].
pub async fn to_rvz<P: AsRef<Path>, Q: AsRef<Path>>(
    source: P,
    destination: Q,
    progress_bar: &ProgressBar,
    compression_algorithm: &RvzCompressionAlgorithm,
    compression_level: usize,
    block_size: usize,
    _scrub: bool,
) -> Result<()> {
    write(
        source,
        destination,
        progress_bar,
        FormatOptions {
            format: Format::Rvz,
            compression: compression(compression_algorithm, compression_level),
            // The setting is in KiB, nod wants bytes
            block_size: (block_size * 1024) as u32,
        },
    )
    .await
}

/// Convert an ISO to WBFS.
pub async fn to_wbfs<P: AsRef<Path>, Q: AsRef<Path>>(
    source: P,
    destination: Q,
    progress_bar: &ProgressBar,
) -> Result<()> {
    write(
        source,
        destination,
        progress_bar,
        FormatOptions::new(Format::Wbfs),
    )
    .await
}

/// Run a disc writer to completion, streaming its output to `destination`.
async fn write<P: AsRef<Path>, Q: AsRef<Path>>(
    source: P,
    destination: Q,
    progress_bar: &ProgressBar,
    options: FormatOptions,
) -> Result<()> {
    let source = source.as_ref().to_path_buf();
    let destination = destination.as_ref().to_path_buf();
    let progress_bar = progress_bar.clone();
    spawn_blocking(move || {
        let disc = open(&source)?;
        let writer = DiscWriter::new(disc, &options)
            .with_context(|| format!("Failed to set up the {} writer", options.format))?;
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
        // The writer reports how far through the source it is, which for a
        // compressing format bears no relation to the bytes written out
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
        // RVZ and WBFS only know their header once every block has been written
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

/// What `info` calls this backend.
pub const BACKEND_NAME: &str = "nod";

/// nod's `ScrubLevel` only reaches its WBFS and CISO writers, so there is
/// nothing for `RVZ_SCRUB` to map onto.
pub const SUPPORTS_SCRUB: bool = false;

/// Reported by `info`. nod exposes no version constant, and a linked library
/// has no version to query at runtime the way a subprocess does.
pub async fn get_version() -> Result<String> {
    Ok(String::from("built-in"))
}

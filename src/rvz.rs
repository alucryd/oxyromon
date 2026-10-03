//! RVZ, and the GameCube/Wii disc handling it shares with WBFS, by way of nod.

use super::common::*;
use super::config::*;
use super::mimetype::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use anyhow::{Context, Result, bail};
use indicatif::ProgressBar;
use nod::common::{Compression, Format};
use nod::read::{DiscOptions, DiscReader, PartitionEncryption};
use nod::write::{DiscWriter, DiscWriterWeight, FormatOptions, ProcessOptions, ScrubLevel};
use sqlx::SqliteConnection;
use std::fs::File;
use std::io::{Seek, Write};
use std::path::Path;
use strum::{Display, EnumString, VariantNames};
use tokio::task::spawn_blocking;

pub const RVZ_BLOCK_SIZE_RANGE: [usize; 2] = [32, 2048];
pub const RVZ_COMPRESSION_LEVEL_RANGE: [usize; 2] = [1, 22];

#[derive(Display, PartialEq, EnumString, VariantNames)]
#[strum(serialize_all = "lowercase")]
pub enum RvzCompressionAlgorithm {
    None,
    Zstd,
    Bzip2,
    Lzma,
    Lzma2,
}

pub struct RvzRomfile {
    pub romfile: CommonRomfile,
}

impl GetRomfile for RvzRomfile {
    fn romfile(&self) -> &CommonRomfile {
        &self.romfile
    }
}

impl HashAndSize for RvzRomfile {
    async fn get_hash_and_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> Result<(String, u64)> {
        progress_bar.reset();
        progress_bar.set_message(format!(
            "Computing {} ({}/{})",
            hash_algorithm, position, total
        ));

        // Hashed as it decodes, with no ISO written out
        let _ = connection;
        let (hash, size) = hash_disc(&self.romfile.path, progress_bar, hash_algorithm).await?;

        progress_bar.set_message("");

        Ok((hash, size))
    }
}

impl Check for RvzRomfile {
    async fn check(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        roms: &[&Rom],
    ) -> Result<()> {
        print_action(progress_bar, &format!("Checking \"{}\"", self.romfile));

        // Headers are a cartridge era concern and no GameCube or Wii DAT
        // declares one, but honour it the long way round if one ever shows up
        if header.is_none() {
            let rom = roms[0];
            let hash_algorithm = get_hash_algorithm(rom)?;
            let (hash, size) = self
                .get_hash_and_size(connection, progress_bar, 1, 1, &hash_algorithm)
                .await?;
            return compare_hash_and_size(rom, &hash, size, &hash_algorithm);
        }

        let tmp_directory = create_tmp_directory(connection).await?;
        let iso_romfile = self.to_iso(progress_bar, &tmp_directory.path()).await?;
        iso_romfile
            .romfile
            .check(connection, progress_bar, header, roms)
            .await?;
        Ok(())
    }
}

impl ToIso for RvzRomfile {
    async fn to_iso<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<IsoRomfile> {
        progress_bar.set_message("Extracting rvz");

        print_action(
            progress_bar,
            &format!(
                "Extracting \"{}\"",
                self.romfile.path.file_name().unwrap().to_str().unwrap()
            ),
        );

        let path = destination_directory
            .as_ref()
            .join(self.romfile.path.file_name().unwrap())
            .with_extension(ISO_EXTENSION);

        extract_iso(&self.romfile.path, &path, progress_bar).await?;

        stop_action(progress_bar);

        CommonRomfile::from_path(&path)?.as_iso()
    }
}

pub trait ToRvz {
    async fn to_rvz<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        compression_algorithm: &RvzCompressionAlgorithm,
        compression_level: usize,
        block_size: usize,
        scrub: bool,
    ) -> Result<RvzRomfile>;
}

impl ToRvz for IsoRomfile {
    async fn to_rvz<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        compression_algorithm: &RvzCompressionAlgorithm,
        compression_level: usize,
        block_size: usize,
        scrub: bool,
    ) -> Result<RvzRomfile> {
        progress_bar.set_message("Creating rvz");

        let path = destination_directory
            .as_ref()
            .join(self.romfile.path.file_name().unwrap())
            .with_extension(RVZ_EXTENSION);

        print_action(
            progress_bar,
            &format!(
                "Creating \"{}\"",
                path.file_name().unwrap().to_str().unwrap()
            ),
        );

        if scrub {
            print_warning(
                progress_bar,
                "RVZ_SCRUB is unsupported by nod, writing unscrubbed",
            );
        }

        write_rvz(
            &self.romfile.path,
            &path,
            progress_bar,
            compression_algorithm,
            compression_level,
            block_size,
        )
        .await?;

        stop_action(progress_bar);

        CommonRomfile::from_path(&path)?.as_rvz()
    }
}

pub trait AsRvz {
    fn as_rvz(self) -> Result<RvzRomfile>;
}

impl AsRvz for CommonRomfile {
    fn as_rvz(self) -> Result<RvzRomfile> {
        if self
            .path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase()
            != RVZ_EXTENSION
        {
            bail!("Not a valid rvz");
        }
        Ok(RvzRomfile { romfile: self })
    }
}

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
pub(super) async fn extract_iso<P: AsRef<Path>, Q: AsRef<Path>>(
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
async fn hash_disc<P: AsRef<Path>>(
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

/// Convert an ISO to RVZ. nod cannot scrub RVZ.
async fn write_rvz<P: AsRef<Path>, Q: AsRef<Path>>(
    source: P,
    destination: Q,
    progress_bar: &ProgressBar,
    compression_algorithm: &RvzCompressionAlgorithm,
    compression_level: usize,
    block_size: usize,
) -> Result<()> {
    write_disc(
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

/// Run a disc writer to completion, streaming its output to `destination`.
pub(super) async fn write_disc<P: AsRef<Path>, Q: AsRef<Path>>(
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

/// Reported by `info`.
pub async fn get_version() -> Result<String> {
    Ok(String::from("built-in"))
}

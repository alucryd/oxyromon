//! RVZ, Dolphin's compressed GameCube and Wii disc image, by way of rvz-rs.
//!
//! Only writing an RVZ from a Wii ISO needs a key, the Wii common key, which
//! oxyromon does not ship: `import-keys` stores it. Reading an RVZ, or
//! rewriting one, needs none, and neither does a GameCube disc.

use super::common::*;
use super::config::*;
use super::import_keys::WIIU_KEYS_FILE;
use super::mimetype::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use anyhow::{Context, Result, bail};
use indicatif::ProgressBar;
use rvz_rs::{Compression, Options, Reader};
use sqlx::SqliteConnection;
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

        let (input, output) = (self.romfile.path.clone(), path.clone());
        run_blocking(progress_bar, input.metadata()?.len(), move |progress| {
            rvz_rs::decompress(&input, &output, progress)
        })
        .await
        .with_context(|| format!("Failed to extract \"{}\"", self.romfile.path.display()))?;

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

        let options = options(compression_algorithm, compression_level, block_size);
        let keys = load_wii_keys()?;
        let (input, output) = (self.romfile.path.clone(), path.clone());
        run_blocking(progress_bar, input.metadata()?.len(), move |progress| {
            rvz_rs::compress(&input, &output, &options, &keys, progress)
        })
        .await
        .map_err(|error| match error.downcast_ref() {
            Some(rvz_rs::Error::MissingKey(_)) => {
                error.context("A Wii disc needs the Wii common key: run import-keys")
            }
            _ => error,
        })
        .with_context(|| format!("Failed to create \"{}\"", path.display()))?;

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

/// The Wii common key, which `import-keys` stores with the Wii U keys, as it
/// comes from a Wii U's `otp.bin` as often as from a Wii. Only writing an RVZ
/// or a WBFS from a Wii ISO needs it; without it, that fails, and nothing
/// else does.
pub(super) fn load_wii_keys() -> Result<Vec<rvz_rs::Key>> {
    let path = get_data_directory()?.join(WIIU_KEYS_FILE);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    rvz_rs::load_keys(&path).with_context(|| format!("Failed to read \"{}\"", path.display()))
}

/// Map oxyromon's configured algorithm and level onto rvz-rs's options.
///
/// `RVZ_COMPRESSION_LEVEL` ranges over 1..=22, which only Zstandard accepts in
/// full: the others cap at 9. The level is clamped rather than rejected, so an
/// existing configuration keeps working instead of failing the conversion.
fn options(algorithm: &RvzCompressionAlgorithm, level: usize, block_size: usize) -> Options {
    let compression = match algorithm {
        RvzCompressionAlgorithm::None => Compression::None,
        RvzCompressionAlgorithm::Bzip2 => Compression::Bzip2,
        RvzCompressionAlgorithm::Lzma => Compression::Lzma,
        RvzCompressionAlgorithm::Lzma2 => Compression::Lzma2,
        RvzCompressionAlgorithm::Zstd => Compression::Zstd,
    };
    let levels = compression.levels();
    Options {
        compression,
        level: (level as i32).clamp(*levels.start(), *levels.end()),
        // The setting is in KiB, rvz-rs wants bytes and a power of two
        chunk_size: (block_size.next_power_of_two() * 1024) as u32,
        scrub: false,
    }
}

/// Hash an RVZ's disc without writing the decoded ISO anywhere.
///
/// This is what the extract-then-hash path costs on a dual layer Wii disc:
/// several gigabytes written to the temp directory and read straight back. The
/// decoder is just a reader, so the digest can consume it directly.
async fn hash_disc(
    source: &Path,
    progress_bar: &ProgressBar,
    hash_algorithm: &HashAlgorithm,
) -> Result<(String, u64)> {
    let source = source.to_path_buf();
    let hash_algorithm = *hash_algorithm;
    let progress_bar = progress_bar.clone();
    spawn_blocking(move || {
        let mut disc = Reader::open(&source)
            .with_context(|| format!("Failed to open \"{}\"", source.display()))?;
        progress_bar.set_style(get_bytes_progress_style());
        progress_bar.set_length(disc.size());
        hash_reader(&mut disc, &progress_bar, &hash_algorithm)
    })
    .await
    .context("Disc reader task failed")?
}

/// Reported by `info`.
pub async fn get_version() -> Result<String> {
    Ok(String::from("built-in"))
}

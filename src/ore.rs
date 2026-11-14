//! ORE, oxyROMon's own compressed disc image, by way of ore-rs.
//!
//! An ORE holds a GameCube or Wii disc, or a CD as its cue sheet and tracks:
//! a CD's cue sheet is inside it, so the cue's ROM points to the ORE as its
//! tracks' do. Like RVZ, only writing one from a Wii ISO needs the Wii common
//! key, which `import-keys` stores; an ORE keeps its title keys, so reading
//! one needs none.

use super::common::*;
use super::config::*;
use super::import_keys::WIIU_KEYS_FILE;
use super::mimetype::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use anyhow::{Context, Result, bail};
use indicatif::ProgressBar;
use ore_rs::{Codec, Options, Profile, Reader};
use sqlx::SqliteConnection;
use std::io::Read;
use std::path::Path;
use strum::{Display, EnumString, VariantNames};
use tokio::task::spawn_blocking;

#[derive(Display, PartialEq, EnumString, VariantNames)]
#[strum(serialize_all = "lowercase")]
pub enum OreCompressionAlgorithm {
    Zstd,
    Lzma2,
}

#[derive(Display, PartialEq, EnumString, VariantNames)]
#[strum(serialize_all = "lowercase")]
pub enum OreProfile {
    Stream,
    Archive,
}

pub struct OreRomfile {
    pub romfile: CommonRomfile,
}

impl GetRomfile for OreRomfile {
    fn romfile(&self) -> &CommonRomfile {
        &self.romfile
    }
}

impl HashAndSize for OreRomfile {
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

impl Check for OreRomfile {
    async fn check(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        roms: &[&Rom],
    ) -> Result<()> {
        print_action(progress_bar, &format!("Checking \"{}\"", self.romfile));

        // A CD's files, written out and checked one by one
        if self.is_cd()? {
            let tmp_directory = create_tmp_directory(connection).await?;
            let cue_name = roms
                .iter()
                .find(|rom| rom.name.ends_with(CUE_EXTENSION))
                .map(|rom| rom.name.as_str());
            let cue_bin_romfile = self
                .to_cue_bin_files(progress_bar, &tmp_directory.path(), cue_name)
                .await?;
            let files = [
                std::slice::from_ref(&cue_bin_romfile.cue_romfile),
                cue_bin_romfile.bin_romfiles.as_slice(),
            ]
            .concat();
            for rom in roms {
                let file = files
                    .iter()
                    .find(|file| file.path.file_name().unwrap().to_str() == Some(&rom.name))
                    .with_context(|| format!("\"{}\" is not in \"{}\"", rom.name, self.romfile))?;
                file.check(connection, progress_bar, &None, &[rom]).await?;
            }
            return Ok(());
        }

        // No GameCube or Wii DAT declares a header, but honour one the long
        // way round if one ever shows up
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

impl ToIso for OreRomfile {
    async fn to_iso<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<IsoRomfile> {
        progress_bar.set_message("Extracting ore");

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
            ore_rs::decompress(&input, &output, progress)
        })
        .await
        .with_context(|| format!("Failed to extract \"{}\"", self.romfile.path.display()))?;

        stop_action(progress_bar);

        CommonRomfile::from_path(&path)?.as_iso()
    }
}

impl OreRomfile {
    /// Whether it holds a CD rather than a disc image.
    pub fn is_cd(&self) -> Result<bool> {
        Ok(cd_files(&self.romfile.path)?.is_some())
    }

    /// Write a CD ORE out as its cue sheet, named `cue_name` or as it was
    /// compressed, its tracks, under the names the cue sheet gives them, and
    /// an SBI it holds beside them.
    pub async fn to_cue_bin_files<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        cue_name: Option<&str>,
    ) -> Result<CueBinRomfile> {
        let Some(mut names) = cd_files(&self.romfile.path)? else {
            bail!("\"{}\" does not hold a CD", self.romfile);
        };
        progress_bar.set_message("Extracting ore");
        print_action(
            progress_bar,
            &format!(
                "Extracting \"{}\"",
                self.romfile.path.file_name().unwrap().to_str().unwrap()
            ),
        );

        if let Some(cue_name) = cue_name {
            names[0] = cue_name.to_string();
        }
        let directory = destination_directory.as_ref();
        let (input, output) = (self.romfile.path.clone(), directory.join(&names[0]));
        run_blocking(progress_bar, input.metadata()?.len(), move |progress| {
            ore_rs::decompress(&input, &output, progress)
        })
        .await
        .with_context(|| format!("Failed to extract \"{}\"", self.romfile.path.display()))?;

        stop_action(progress_bar);

        let mut bin_romfiles = vec![];
        for name in &names[1..] {
            bin_romfiles.push(CommonRomfile::from_path(&directory.join(name))?);
        }
        CommonRomfile::from_path(&directory.join(&names[0]))?.as_cue_bin(bin_romfiles)
    }
}

impl OreRomfile {
    /// Put the cue sheet at `cue` in this CD ORE in place of its own, under
    /// `name`, writing its metadata alone again: the cue sheet must describe
    /// the tracks it holds.
    pub async fn replace_cue(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        cue: &Path,
        name: &str,
    ) -> Result<()> {
        // ore-rs names the cue sheet after its file
        let tmp_directory = create_tmp_directory(connection).await?;
        let named = tmp_directory.path().join(name);
        tokio::fs::copy(cue, &named).await?;

        print_action(
            progress_bar,
            &format!("Writing \"{}\" into \"{}\"", name, self.romfile),
        );
        let input = self.romfile.path.clone();
        run_blocking(progress_bar, input.metadata()?.len(), move |progress| {
            ore_rs::replace_cue(&input, &input, &named, progress)
        })
        .await
        .with_context(|| format!("Failed to write \"{}\" into \"{}\"", name, self.romfile))?;
        stop_action(progress_bar);
        Ok(())
    }
}

pub trait ToOre {
    async fn to_ore<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        compression_algorithm: &OreCompressionAlgorithm,
        profile: &OreProfile,
    ) -> Result<OreRomfile>;
}

impl ToOre for IsoRomfile {
    async fn to_ore<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        compression_algorithm: &OreCompressionAlgorithm,
        profile: &OreProfile,
    ) -> Result<OreRomfile> {
        let size = self.romfile.path.metadata()?.len();
        compress(
            progress_bar,
            &self.romfile.path,
            destination_directory.as_ref(),
            compression_algorithm,
            profile,
            size,
        )
        .await
    }
}

/// Compress `source`, an ISO or a cue sheet, into `destination_directory`,
/// `size` the input bytes the progress bar counts.
async fn compress(
    progress_bar: &ProgressBar,
    source: &Path,
    destination_directory: &Path,
    compression_algorithm: &OreCompressionAlgorithm,
    profile: &OreProfile,
    size: u64,
) -> Result<OreRomfile> {
    progress_bar.set_message("Creating ore");

    let path = destination_directory
        .join(source.file_name().unwrap())
        .with_extension(ORE_EXTENSION);

    print_action(
        progress_bar,
        &format!(
            "Creating \"{}\"",
            path.file_name().unwrap().to_str().unwrap()
        ),
    );

    let options = Options {
        profile: match profile {
            OreProfile::Stream => Profile::Stream,
            OreProfile::Archive => Profile::Archive,
        },
        codec: match compression_algorithm {
            OreCompressionAlgorithm::Zstd => Codec::Zstd,
            OreCompressionAlgorithm::Lzma2 => Codec::Lzma2,
        },
    };
    let keys = load_wii_keys()?;
    let (input, output) = (source.to_path_buf(), path.clone());
    run_blocking(progress_bar, size, move |progress| {
        ore_rs::compress(&input, &output, &options, &keys, progress)
    })
    .await
    .map_err(|error| match error.downcast_ref() {
        Some(ore_rs::Error::MissingKey(_)) => {
            error.context("A Wii disc needs the Wii common key: run import-keys")
        }
        _ => error,
    })
    .with_context(|| format!("Failed to create \"{}\"", path.display()))?;

    stop_action(progress_bar);

    CommonRomfile::from_path(&path)?.as_ore()
}

impl ToOre for CueBinRomfile {
    async fn to_ore<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        compression_algorithm: &OreCompressionAlgorithm,
        profile: &OreProfile,
    ) -> Result<OreRomfile> {
        let size: u64 = self
            .bin_romfiles
            .iter()
            .map(|romfile| romfile.path.metadata().map(|metadata| metadata.len()))
            .sum::<std::io::Result<u64>>()?;
        // ore-rs reads the tracks the cue sheet names from beside it
        compress(
            progress_bar,
            &self.cue_romfile.path,
            destination_directory.as_ref(),
            compression_algorithm,
            profile,
            size,
        )
        .await
    }
}

pub trait AsOre {
    fn as_ore(self) -> Result<OreRomfile>;
}

impl AsOre for CommonRomfile {
    fn as_ore(self) -> Result<OreRomfile> {
        if self
            .path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase()
            != ORE_EXTENSION
        {
            bail!("Not a valid ore");
        }
        Ok(OreRomfile { romfile: self })
    }
}

/// A CD ORE's files as the DAT names them, its cue sheet first, then its
/// tracks in order, from the hashes it records; `None` for a disc image.
fn cd_files(path: &Path) -> Result<Option<Vec<String>>> {
    let reader =
        Reader::inspect(path).with_context(|| format!("Failed to open \"{}\"", path.display()))?;
    if !reader.is_cd() {
        return Ok(None);
    }
    let mut names: Vec<String> = vec![];
    for (view, _, _) in reader.identity() {
        let extension = Path::new(view)
            .extension()
            .map(|extension| extension.to_string_lossy().to_lowercase());
        if view != "logical"
            && extension.as_deref() != Some("sbi")
            && !names.iter().any(|name| name == view)
        {
            names.push(view.to_string());
        }
    }
    // stable, so the tracks keep their order
    names.sort_by_key(|name| !name.to_lowercase().ends_with(&format!(".{CUE_EXTENSION}")));
    if names
        .first()
        .is_none_or(|name| !name.to_lowercase().ends_with(CUE_EXTENSION))
    {
        bail!("\"{}\" records no cue sheet", path.display());
    }
    Ok(Some(names))
}

/// The Wii common key, where `import-keys` stores it, as ore-rs reads it.
fn load_wii_keys() -> Result<Vec<ore_rs::Key>> {
    let path = get_data_directory()?.join(WIIU_KEYS_FILE);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    ore_rs::load_keys(&path).with_context(|| format!("Failed to read \"{}\"", path.display()))
}

/// An ORE's disc as a stream, for hashing.
struct DiscReader {
    reader: Reader,
    offset: u64,
}

impl Read for DiscReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let len = buf
            .len()
            .min(self.reader.size().saturating_sub(self.offset) as usize);
        self.reader
            .read_at(self.offset, &mut buf[..len])
            .map_err(std::io::Error::other)?;
        self.offset += len as u64;
        Ok(len)
    }
}

/// Hash an ORE's disc without writing the decoded ISO anywhere.
async fn hash_disc(
    source: &Path,
    progress_bar: &ProgressBar,
    hash_algorithm: &HashAlgorithm,
) -> Result<(String, u64)> {
    let source = source.to_path_buf();
    let hash_algorithm = *hash_algorithm;
    let progress_bar = progress_bar.clone();
    spawn_blocking(move || {
        let reader = Reader::open(&source)
            .with_context(|| format!("Failed to open \"{}\"", source.display()))?;
        if reader.is_cd() {
            bail!("\"{}\" holds a CD, not yet supported", source.display());
        }
        progress_bar.set_style(get_bytes_progress_style());
        progress_bar.set_length(reader.size());
        hash_reader(
            &mut DiscReader { reader, offset: 0 },
            &progress_bar,
            &hash_algorithm,
        )
    })
    .await
    .context("Disc reader task failed")?
}

/// Reported by `info`.
pub async fn get_version() -> Result<String> {
    Ok(String::from("built-in"))
}

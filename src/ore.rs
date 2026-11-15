//! ORE, oxyROMon's own compressed disc image, by way of ore-rs.
//!
//! An ORE holds a disc image, a cartridge's ROM under its name, or a CD as
//! its cue sheet and tracks: a CD's cue sheet is inside it, so the cue's ROM
//! points to the ORE as its tracks' do. Like RVZ, only writing one from a Wii ISO needs the Wii common
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
use indicatif::{HumanBytes, ProgressBar};
use ore_rs::{Codec, Options, Profile, Reader, StoreBatch, StoreOptions};
use sqlx::SqliteConnection;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use strum::{Display, EnumString, VariantNames};
use tokio::task::spawn_blocking;

#[derive(Display, PartialEq, EnumString, VariantNames)]
#[strum(serialize_all = "lowercase")]
pub enum OreCompressionAlgorithm {
    /// LZMA2 for a cartridge's ROM, small and read whole, Zstandard for a
    /// disc, the quicker to read
    Auto,
    Zstd,
    Lzma2,
}

#[derive(Display, PartialEq, EnumString, VariantNames)]
#[strum(serialize_all = "lowercase")]
pub enum OreProfile {
    Stream,
    Archive,
}

/// How OREs go into a system's store: a run's at once, which writes the
/// least, or a game's at a time, each committed with its game.
#[derive(Display, PartialEq, EnumString, VariantNames)]
#[strum(serialize_all = "lowercase")]
pub enum OreStoreBatch {
    Run,
    Game,
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

    /// Whether it holds a cartridge's ROM rather than a disc.
    pub fn is_rom(&self) -> Result<bool> {
        Ok(Reader::inspect(&self.romfile.path)?.file_name().is_some())
    }

    /// Write a cartridge's ORE out as its ROM, named `name`.
    pub async fn to_rom<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        name: &str,
    ) -> Result<CommonRomfile> {
        progress_bar.set_message("Extracting ore");
        print_action(progress_bar, &format!("Extracting \"{}\"", self.romfile));
        let path = destination_directory.as_ref().join(name);
        let (input, output) = (self.romfile.path.clone(), path.clone());
        run_blocking(progress_bar, input.metadata()?.len(), move |progress| {
            ore_rs::decompress(&input, &output, progress)
        })
        .await
        .with_context(|| format!("Failed to extract \"{}\"", self.romfile.path.display()))?;
        stop_action(progress_bar);
        CommonRomfile::from_path(&path)
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

/// A cartridge's ROM, which ore-rs takes if its header says it is one it
/// knows.
impl ToOre for CommonRomfile {
    async fn to_ore<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        compression_algorithm: &OreCompressionAlgorithm,
        profile: &OreProfile,
    ) -> Result<OreRomfile> {
        let size = self.path.metadata()?.len();
        compress(
            progress_bar,
            &self.path,
            destination_directory.as_ref(),
            compression_algorithm,
            profile,
            size,
        )
        .await
    }
}

/// Compress `source`, an ISO, a ROM or a cue sheet, into `destination_directory`,
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
            OreCompressionAlgorithm::Auto => ore_rs::default_codec(source)?,
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

/// The store a system's thin OREs share: `.ore-store` in its directory,
/// which every folder beneath finds.
pub async fn system_store(connection: &mut SqliteConnection, system: &System) -> Result<PathBuf> {
    Ok(get_system_directory(connection, system)
        .await?
        .join(ore_rs::STORE_DIR))
}

/// Bring the OREs among `romfiles` in line with the system's `ORE_STORE`:
/// standalone ones attached to its store when it is on, thin ones detached
/// when it is off. An ORE turns thin only once its batch is committed, a
/// run's or a game's as `ORE_STORE_BATCH` says, so it is whole at its path
/// throughout, and the database, which only knows its path, never changes.
pub async fn sync_store(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    romfiles: &[Romfile],
    compression_algorithm: &OreCompressionAlgorithm,
    profile: &OreProfile,
) -> Result<()> {
    let store_on = get_bool(connection, "ORE_STORE", Some(system.id)).await;
    let per_game = OreStoreBatch::from_str(
        &get_string(connection, "ORE_STORE_BATCH", Some(system.id))
            .await
            .unwrap(),
    )
    .unwrap()
        == OreStoreBatch::Game;
    let profile = match profile {
        OreProfile::Stream => Profile::Stream,
        OreProfile::Archive => Profile::Archive,
    };

    let mut moving = Vec::new();
    for romfile in romfiles {
        let path = romfile.as_common(connection).await?.path;
        if path
            .extension()
            .is_none_or(|extension| extension != ORE_EXTENSION)
        {
            continue;
        }
        let reader = Reader::inspect(&path)
            .with_context(|| format!("Failed to read \"{}\"", path.display()))?;
        if reader.store_uuid().is_some() != store_on {
            // Left to ORE, the codec it was written with
            let codec = match compression_algorithm {
                OreCompressionAlgorithm::Auto => reader.codec(),
                OreCompressionAlgorithm::Zstd => Codec::Zstd,
                OreCompressionAlgorithm::Lzma2 => Codec::Lzma2,
            };
            moving.push((path, Options { profile, codec }));
        }
    }
    if moving.is_empty() {
        return Ok(());
    }

    let store = system_store(connection, system).await?;
    if store_on && !store.is_dir() {
        ore_rs::create_store(&store, &StoreOptions::default())
            .with_context(|| format!("Failed to create \"{}\"", store.display()))?;
    }
    print_action(
        progress_bar,
        &format!(
            "{} {} ORE(s) {} \"{}\"",
            if store_on { "Moving" } else { "Taking" },
            moving.len(),
            if store_on { "into" } else { "out of" },
            store.display()
        ),
    );
    let size = moving
        .iter()
        .map(|(path, _)| path.metadata().map(|metadata| metadata.len()))
        .sum::<std::io::Result<u64>>()?;
    run_blocking(progress_bar, size, move |progress| -> ore_rs::Result<()> {
        if !store_on {
            for (path, options) in &moving {
                ore_rs::detach(path, path, options, progress)?;
            }
            return Ok(());
        }
        let mut batch: Option<StoreBatch> = None;
        for (path, options) in &moving {
            let open = match &mut batch {
                Some(open) => open,
                None => batch.insert(StoreBatch::open(&store, options)?),
            };
            // Over itself, landing once the batch is committed
            open.attach(path, path, options, progress)?;
            if per_game {
                batch.take().unwrap().commit()?;
            }
        }
        if let Some(batch) = batch {
            batch.commit()?;
        }
        Ok(())
    })
    .await
    .context("Failed to sync the system's ORE store")?;
    stop_action(progress_bar);
    Ok(())
}

/// Check a system's store whole, repairing what its parity covers, and
/// fail naming the games past repair.
pub async fn check_store(progress_bar: &ProgressBar, store: &Path) -> Result<()> {
    print_action(progress_bar, &format!("Checking \"{}\"", store.display()));
    let path = store.to_path_buf();
    let checked = run_blocking(progress_bar, 0, move |_| ore_rs::check_store(&path, true))
        .await
        .with_context(|| format!("Failed to check \"{}\"", store.display()))?;
    stop_action(progress_bar);
    if checked.repaired > 0 {
        print_warning(
            progress_bar,
            &format!(
                "Repaired {} damaged block(s) in \"{}\"",
                checked.repaired,
                store.display()
            ),
        );
    }
    if checked.lost_groups > 0 {
        let games: Vec<String> = checked
            .lost_games
            .iter()
            .map(|game| game.display().to_string())
            .collect();
        bail!(
            "\"{}\" has {} group(s) past repair, used by: {}",
            store.display(),
            checked.lost_groups,
            games.join(", ")
        );
    }
    Ok(())
}

/// Compact a system's store: drop what no game beneath it uses any more,
/// and pack small groups together.
pub async fn compact_store(
    progress_bar: &ProgressBar,
    store: &Path,
    profile: &OreProfile,
) -> Result<()> {
    print_action(progress_bar, &format!("Compacting \"{}\"", store.display()));
    let options = Options {
        profile: match profile {
            OreProfile::Stream => Profile::Stream,
            OreProfile::Archive => Profile::Archive,
        },
        ..Options::default()
    };
    let path = store.to_path_buf();
    // How dead a segment has to be before its live chunks move, as orers
    let compacted = run_blocking(progress_bar, 0, move |_| {
        ore_rs::compact(&path, &options, 0.5, false)
    })
    .await
    .with_context(|| format!("Failed to compact \"{}\"", store.display()))?;
    stop_action(progress_bar);
    if compacted.after < compacted.before {
        print_success(
            progress_bar,
            &format!(
                "Compacted \"{}\" from {} to {}",
                store.display(),
                HumanBytes(compacted.before),
                HumanBytes(compacted.after)
            ),
        );
    }
    Ok(())
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

//! ORE, oxyROMon's own compressed disc image, by way of ore-rs.
//!
//! An ORE holds a disc image, a cartridge's ROM under its name, or a CD as
//! its cue sheet and tracks: a CD's cue sheet is inside it, so the cue's ROM
//! points to the ORE as its tracks' do. Like RVZ, only writing one from a Wii ISO needs the Wii common
//! key, which `import-keys` stores; an ORE keeps its title keys, so reading
//! one needs none.

use super::common::*;
use super::config::*;
use super::mimetype::*;
use super::model::*;
use super::progress::*;
#[cfg(feature = "server")]
use super::query::StoreIndex;
use super::util::*;
use anyhow::{Context, Result, bail};
use indexmap::IndexMap;
use indicatif::{HumanBytes, ProgressBar};
use ore_rs::{Codec, Options, Profile, Reader, StoreBatch, StoreOptions};
use sqlx::SqliteConnection;
#[cfg(any(feature = "mcp", test))]
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::str::FromStr;
#[cfg(any(feature = "mcp", test))]
use std::sync::Arc;
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

/// Parity shards a store's segment gets against its 239 data shards: 16
/// adds 6.7% and rebuilds up to 16 damaged blocks in a row of 255, 0 adds
/// nothing and rebuilds nothing.
pub const ORE_STORE_PARITY_RANGE: [usize; 2] = [0, 16];

/// The ID of the system whose ORE store settings, `ORE_STORE`,
/// `ORE_STORE_PARITY`, `ORE_STORE_BATCH` and `ORE_PROFILE`, `system`
/// follows. With `GROUP_SUBSYSTEMS`, systems of separate DATs share their
/// directory, and so one store: its settings are the base system's, the one
/// named as the directory is, so that each system's run does not set the
/// store back to its own. Without a base system imported, or without
/// grouping, a system's settings are its own.
pub async fn store_settings_system_id(
    connection: &mut SqliteConnection,
    system: &System,
) -> Result<i64> {
    if !get_bool(connection, "GROUP_SUBSYSTEMS", Some(system.id)).await {
        return Ok(system.id);
    }
    let directory = get_system_directory(connection, system).await?;
    let base_name = directory.file_name().and_then(|name| name.to_str());
    for other in crate::database::find_systems(connection).await {
        let name = other.custom_name.as_deref().unwrap_or(other.name.trim());
        if Some(name) == base_name && get_system_directory(connection, &other).await? == directory {
            return Ok(other.id);
        }
    }
    Ok(system.id)
}

/// The system's `ORE_STORE_PARITY`.
pub async fn store_parity(connection: &mut SqliteConnection, system_id: i64) -> u8 {
    get_integer(connection, "ORE_STORE_PARITY", Some(system_id))
        .await
        .map_or(16, |parity| parity.min(16) as u8)
}

/// Make the store at `store` with `parity` if missing, or set an existing
/// one's: a segment sealed from now on takes it, and the next compaction
/// seals those sealed otherwise again.
fn prepare_store(store: &Path, parity: u8) -> ore_rs::Result<()> {
    if store.is_dir() {
        return ore_rs::set_parity(store, parity).map(|_| ());
    }
    ore_rs::create_store(
        store,
        &StoreOptions {
            parity,
            ..StoreOptions::default()
        },
    )
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

        // A Wii U title's files, written out as its folder and checked one
        // by one
        if self.is_title()? {
            let tmp_directory = create_tmp_directory(connection).await?;
            let folder = tmp_directory.path().join("title");
            for (file, rom) in self
                .to_title_files(progress_bar, &folder, roms)
                .await?
                .iter()
                .zip(roms)
            {
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

    /// Whether it holds a Wii U eShop title, a NUS folder's files.
    pub fn is_title(&self) -> Result<bool> {
        Ok(Reader::inspect(&self.romfile.path)?.is_title())
    }

    /// Write a Wii U title's ORE out as its NUS folder, `folder`, its ticket
    /// beside its files: the files `roms` name, in their order.
    pub async fn to_title_files(
        &self,
        progress_bar: &ProgressBar,
        folder: &Path,
        roms: &[&Rom],
    ) -> Result<Vec<CommonRomfile>> {
        self.extract_title(progress_bar, folder).await?;
        roms.iter()
            .map(|rom| {
                let path = folder.join(&rom.name);
                if !path.is_file() {
                    bail!("\"{}\" is not in \"{}\"", rom.name, self.romfile);
                }
                CommonRomfile::from_path(&path)
            })
            .collect()
    }

    /// Write a Wii U title's ORE out as its NUS folder, `folder`, every file
    /// it holds, its ticket included.
    pub async fn extract_title(&self, progress_bar: &ProgressBar, folder: &Path) -> Result<()> {
        progress_bar.set_message("Extracting ore");
        print_action(
            progress_bar,
            &format!(
                "Extracting \"{}\"",
                self.romfile.path.file_name().unwrap().to_str().unwrap()
            ),
        );
        let (input, output) = (self.romfile.path.clone(), folder.to_path_buf());
        run_blocking(progress_bar, input.metadata()?.len(), move |progress| {
            ore_rs::decompress(&input, &output, progress)
        })
        .await
        .with_context(|| format!("Failed to extract \"{}\"", self.romfile.path.display()))?;
        stop_action(progress_bar);
        Ok(())
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
            None,
            None,
        )
        .await
    }
}

impl IsoRomfile {
    /// [`ToOre::to_ore`], into `sink` when there is one: a thin ORE, landing
    /// when its batch is committed.
    pub async fn to_ore_into<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        compression_algorithm: &OreCompressionAlgorithm,
        profile: &OreProfile,
        sink: Option<&mut StoreSink>,
    ) -> Result<OreRomfile> {
        let size = self.romfile.path.metadata()?.len();
        compress(
            progress_bar,
            &self.romfile.path,
            destination_directory.as_ref(),
            compression_algorithm,
            profile,
            size,
            None,
            sink,
        )
        .await
    }
}

/// A cartridge's ROM: of `platform` when its system names one, which
/// ore-rs takes whole; otherwise if its header says it is one ore-rs knows.
/// Into `sink` when there is one, as [`IsoRomfile::to_ore_into`].
impl CommonRomfile {
    pub async fn to_ore_rom<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        compression_algorithm: &OreCompressionAlgorithm,
        profile: &OreProfile,
        platform: Option<&'static str>,
        sink: Option<&mut StoreSink>,
    ) -> Result<OreRomfile> {
        let size = self.path.metadata()?.len();
        compress(
            progress_bar,
            &self.path,
            destination_directory.as_ref(),
            compression_algorithm,
            profile,
            size,
            platform,
            sink,
        )
        .await
    }
}

/// The ROMs of `games` by the ORE they are in, so that a game of several
/// disks, an ORE a disk, is taken a disk at a time; a disk not converted
/// yet, as a failed run leaves it, is left out.
pub fn by_ore(
    games: IndexMap<i64, Vec<Rom>>,
    romfiles_by_id: &HashMap<i64, Romfile>,
) -> IndexMap<i64, Vec<Rom>> {
    let mut ores: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    for rom in games.into_values().flatten() {
        let id = rom.romfile_id.unwrap();
        if romfiles_by_id[&id].path.ends_with(ORE_EXTENSION) {
            ores.entry(id).or_default().push(rom);
        }
    }
    ores
}

/// The cartridge or floppy platform ore-rs knows a system's ROMs as, by
/// its DAT's name, any suffix such as `(Headered)` aside: its ROMs are then
/// taken whole, a game of several disks an ORE a disk, the DAT vouching for them, whether or not they carry a header.
pub fn cartridge_platform(system: &System) -> Option<&'static str> {
    let name = system.name.split(" (").next().unwrap_or_default();
    Some(match name {
        "Atari - Atari 2600" | "Atari - 2600" => "a2600",
        "Atari - Atari 5200" | "Atari - 5200" => "a5200",
        "Atari - Atari 7800" | "Atari - 7800" => "a7800",
        "Atari - Atari Jaguar" | "Atari - Jaguar" => "jaguar",
        "Atari - Atari Lynx" | "Atari - Lynx" => "lynx",
        "Atari - Atari ST" | "Atari - ST" => "st",
        "Bandai - WonderSwan" => "ws",
        "Bandai - WonderSwan Color" => "wsc",
        "Coleco - ColecoVision" => "colecovision",
        "Commodore - Amiga" => "amiga",
        "Magnavox - Odyssey 2" | "Magnavox - Odyssey2" => "odyssey2",
        "NEC - PC Engine - TurboGrafx-16" | "NEC - PC Engine - TurboGrafx 16" => "pce",
        "NEC - PC Engine SuperGrafx" => "sgx",
        "Nintendo - Family Computer Disk System" => "fds",
        "Nintendo - Game Boy" => "gb",
        "Nintendo - Game Boy Advance" => "gba",
        "Nintendo - Game Boy Color" => "gbc",
        "Nintendo - Nintendo 64" => "n64",
        "Nintendo - Nintendo 64DD" => "n64dd",
        // A card image; the eShop's DATs list CIAs' contents
        "Nintendo - Nintendo 3DS" | "Nintendo - New Nintendo 3DS"
            if !system.name.contains("(Digital)") =>
        {
            "3ds"
        }
        "Nintendo - Nintendo DS" => "nds",
        "Nintendo - Nintendo DSi" => "dsi",
        "Nintendo - Nintendo Entertainment System" => "nes",
        "Nintendo - Pokemon Mini" => "pokemini",
        "Nintendo - Satellaview" => "satellaview",
        "Nintendo - Sufami Turbo" => "sufami",
        "Nintendo - Super Nintendo Entertainment System" => "snes",
        "Nintendo - Virtual Boy" => "vb",
        "SNK - NeoGeo Pocket" => "ngp",
        "SNK - NeoGeo Pocket Color" => "ngpc",
        "Sega - 32X" => "32x",
        "Sega - Game Gear" => "gg",
        "Sega - Master System - Mark III" => "sms",
        "Sega - Mega Drive - Genesis" => "md",
        "Sega - SG-1000" | "Sega - SG-1000 - SC-3000" => "sg1000",
        _ => return None,
    })
}

/// Compress `source`, an ISO, a ROM or a cue sheet, into `destination_directory`,
/// `size` the input bytes the progress bar counts; into `sink`'s store when
/// there is one, the ORE landing once the sink is committed.
#[allow(clippy::too_many_arguments)]
async fn compress(
    progress_bar: &ProgressBar,
    source: &Path,
    destination_directory: &Path,
    compression_algorithm: &OreCompressionAlgorithm,
    profile: &OreProfile,
    size: u64,
    platform: Option<&'static str>,
    sink: Option<&mut StoreSink>,
) -> Result<OreRomfile> {
    progress_bar.set_message("Creating ore");

    // A folder's name may hold a dot, which is not an extension
    let path = match source.is_dir() {
        true => destination_directory.join(format!(
            "{}.{ORE_EXTENSION}",
            source.file_name().unwrap().to_string_lossy()
        )),
        false => destination_directory
            .join(source.file_name().unwrap())
            .with_extension(ORE_EXTENSION),
    };

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
            OreCompressionAlgorithm::Auto => ore_rs::default_codec(source, platform)?,
            OreCompressionAlgorithm::Zstd => Codec::Zstd,
            OreCompressionAlgorithm::Lzma2 => Codec::Lzma2,
        },
        platform,
    };
    // Taken whole as its system says, but told when its header disagrees
    if let Some(platform) = platform
        && ore_rs::cartridge_header_matches(source, platform)? == Some(false)
    {
        print_warning(
            progress_bar,
            &format!(
                "\"{}\" has no {} header, taken as its DAT says",
                source.file_name().unwrap().to_str().unwrap(),
                platform
            ),
        );
    }
    let keys = load_wii_keys()?;
    let result = match sink {
        Some(sink) => {
            sink.compress(progress_bar, source, &path, options, keys, size)
                .await
        }
        None => {
            let (input, output) = (source.to_path_buf(), path.clone());
            run_blocking(progress_bar, size, move |progress| {
                ore_rs::compress(&input, &output, &options, &keys, progress)
            })
            .await
            .map(|_| ())
        }
    };
    result
        .map_err(|error| match error.downcast_ref() {
            Some(ore_rs::Error::MissingKey(_)) => error.context(
                "A Wii disc needs the Wii common key, a Wii U disc its disc key and the Wii U \
                 common key, a Wii U title the Wii U common key: run import-keys",
            ),
            _ => error,
        })
        .with_context(|| format!("Failed to create \"{}\"", path.display()))?;

    stop_action(progress_bar);

    CommonRomfile::from_path(&path)?.as_ore()
}

/// A Wii U WUX, or a NUS folder holding a title, an update or a DLC, into
/// an ORE in `destination_directory`, named after it; into `sink`'s store
/// when there is one.
pub async fn wiiu_to_ore(
    progress_bar: &ProgressBar,
    source: &Path,
    destination_directory: &Path,
    compression_algorithm: &OreCompressionAlgorithm,
    profile: &OreProfile,
    sink: Option<&mut StoreSink>,
) -> Result<OreRomfile> {
    let size = match source.is_dir() {
        // files only, as orers counts them
        true => std::fs::read_dir(source)?
            .map(|entry| entry.and_then(|entry| entry.metadata()))
            .filter(|metadata| metadata.as_ref().map_or(true, |m| m.is_file()))
            .map(|metadata| metadata.map(|m| m.len()))
            .sum::<std::io::Result<u64>>()?,
        false => source.metadata()?.len(),
    };
    compress(
        progress_bar,
        source,
        destination_directory,
        compression_algorithm,
        profile,
        size,
        None,
        sink,
    )
    .await
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
            None,
            None,
        )
        .await
    }
}

/// A run's OREs written straight into their system's store, as `ORE_STORE`
/// asks, rather than standalone and then attached: compressed once. A thin
/// ORE lands when its batch is committed, a run's or a game's as
/// `ORE_STORE_BATCH` says; until then neither it nor the database exists,
/// so a game is checked and persisted after [`StoreSink::commit`], and a
/// run that fails before leaves the originals and the database as they were.
pub struct StoreSink {
    store: PathBuf,
    parity: u8,
    pub per_game: bool,
    batch: Option<StoreBatch>,
}

impl StoreSink {
    /// The system's sink, or `None` when its `ORE_STORE` is off.
    pub async fn open(connection: &mut SqliteConnection, system: &System) -> Result<Option<Self>> {
        let settings_id = store_settings_system_id(connection, system).await?;
        if !get_bool(connection, "ORE_STORE", Some(settings_id)).await {
            return Ok(None);
        }
        let per_game = OreStoreBatch::from_str(
            &get_string(connection, "ORE_STORE_BATCH", Some(settings_id))
                .await
                .unwrap(),
        )
        .unwrap()
            == OreStoreBatch::Game;
        Ok(Some(Self {
            store: system_store(connection, system).await?,
            parity: store_parity(connection, settings_id).await,
            per_game,
            batch: None,
        }))
    }

    async fn compress(
        &mut self,
        progress_bar: &ProgressBar,
        input: &Path,
        output: &Path,
        options: Options,
        keys: Vec<ore_rs::Key>,
        size: u64,
    ) -> Result<()> {
        let (store, parity, batch) = (self.store.clone(), self.parity, self.batch.take());
        let (input, output) = (input.to_path_buf(), output.to_path_buf());
        let (batch, result) = run_blocking(progress_bar, size, move |progress| {
            let mut batch = match batch {
                Some(batch) => batch,
                None => {
                    prepare_store(&store, parity)?;
                    StoreBatch::open(&store, &options)?
                }
            };
            // A game refused leaves the batch as it was, for the next
            let result = batch.compress(&input, &output, &options, &keys, progress);
            Ok::<_, ore_rs::Error>((batch, result))
        })
        .await?;
        self.batch = Some(batch);
        result.map(|_| ()).map_err(anyhow::Error::from)
    }

    /// Commit what was written, landing its thin OREs.
    pub async fn commit(&mut self, progress_bar: &ProgressBar) -> Result<()> {
        let Some(batch) = self.batch.take() else {
            return Ok(());
        };
        print_action(
            progress_bar,
            &format!("Committing \"{}\"", self.store.display()),
        );
        run_blocking(progress_bar, batch.unsealed(), move |progress| {
            batch.commit(progress)
        })
        .await
        .context("Failed to commit the system's ORE store")?;
        stop_action(progress_bar);
        Ok(())
    }
}

/// Write `ore` to `destination` as a standalone ORE, for it to leave its
/// system: a thin one detached, its chunks out of its store, a standalone
/// one copied.
pub async fn export_ore(
    progress_bar: &ProgressBar,
    ore: &OreRomfile,
    destination: &Path,
) -> Result<()> {
    let reader = Reader::inspect(&ore.romfile.path)
        .with_context(|| format!("Failed to read \"{}\"", ore.romfile))?;
    if reader.store_uuid().is_none() {
        return copy_file(progress_bar, &ore.romfile.path, &destination, false).await;
    }
    print_action(
        progress_bar,
        &format!(
            "Detaching \"{}\" to \"{}\"",
            ore.romfile,
            destination.display()
        ),
    );
    let options = Options {
        profile: reader.profile()?,
        codec: reader.codec(),
        ..Options::default()
    };
    let (input, output) = (ore.romfile.path.clone(), destination.to_path_buf());
    run_blocking(progress_bar, input.metadata()?.len(), move |progress| {
        ore_rs::detach(&input, &output, &options, progress)
    })
    .await
    .with_context(|| format!("Failed to detach \"{}\"", ore.romfile))?;
    stop_action(progress_bar);
    Ok(())
}

/// Write a game's ORE out as the files it holds into `directory`, under the
/// names `roms` give them: a CD's cue sheet and tracks, a cartridge's ROM,
/// or a disc's ISO. A thin ORE reads through its store, where it is.
pub async fn ore_to_files(
    progress_bar: &ProgressBar,
    ore: &OreRomfile,
    roms: &[Rom],
    directory: &Path,
) -> Result<Vec<CommonRomfile>> {
    if ore.is_cd()? {
        let cue_name = roms
            .iter()
            .find(|rom| rom.name.ends_with(CUE_EXTENSION))
            .map(|rom| rom.name.as_str());
        let cue_bin = ore
            .to_cue_bin_files(progress_bar, &directory, cue_name)
            .await?;
        return Ok([vec![cue_bin.cue_romfile], cue_bin.bin_romfiles].concat());
    }
    if ore.is_title()? {
        let roms: Vec<&Rom> = roms.iter().collect();
        return ore.to_title_files(progress_bar, directory, &roms).await;
    }
    let name = &roms.first().context("A game with no ROM")?.name;
    if ore.is_rom()? {
        return Ok(vec![ore.to_rom(progress_bar, &directory, name).await?]);
    }
    let iso = ore.to_iso(progress_bar, &directory).await?.romfile;
    if iso
        .path
        .file_name()
        .and_then(|file_name| file_name.to_str())
        == Some(name)
    {
        return Ok(vec![iso]);
    }
    Ok(vec![
        iso.rename(progress_bar, &directory.join(name), true)
            .await?,
    ])
}

/// The store a system's thin OREs share: `.ore-store` in its directory,
/// which every folder beneath finds.
pub async fn system_store(connection: &mut SqliteConnection, system: &System) -> Result<PathBuf> {
    Ok(get_system_directory(connection, system)
        .await?
        .join(ore_rs::STORE_DIR))
}

/// What the OREs of `romfiles` take from their stores and give them, all
/// together: each game's dedup saving and parity cost, summed. A store
/// shared with other systems, as `GROUP_SUBSYSTEMS` gives, is split by
/// game too, so each system counts only its own part: the parts of a
/// group add up to the whole `store_stats`, and no two systems show the
/// same two numbers. `None` when none of the romfiles lives in a readable
/// store: as the stats, the savings are an extra, no flow fails without.
#[cfg(any(feature = "mcp", test))]
pub async fn store_savings(
    connection: &mut SqliteConnection,
    romfiles: &[Romfile],
) -> Option<ore_rs::Savings> {
    store_savings_in(connection, romfiles, |store| async move {
        spawn_blocking(move || ore_rs::Index::load(&store).ok().map(Arc::new))
            .await
            .ok()
            .flatten()
    })
    .await
}

/// The same savings, with the system's store index already in hand —
/// asked of the request's loader by the field before it weighed anything,
/// as [`actual_size_loaded`] does for the sizes.
#[cfg(feature = "server")]
pub async fn store_savings_loaded(
    connection: &mut SqliteConnection,
    romfiles: &[Romfile],
    store: Option<Arc<StoreIndex>>,
) -> Option<ore_rs::Savings> {
    store_savings_in(connection, romfiles, |path| async {
        match &store {
            Some(store) => store.index_for(&path).await,
            None => spawn_blocking(move || ore_rs::Index::load(&path).ok().map(Arc::new))
                .await
                .ok()
                .flatten(),
        }
    })
    .await
}

/// The savings of `romfiles`, with every store's index obtained through
/// `index_of`: every thin ORE asks its own store, as the sizes do, so a
/// grouped subsystem beneath another directory's store counts its games
/// into that store's numbers, and the index is read once per store.
#[cfg(any(feature = "mcp", test))]
async fn store_savings_in<F, Fut>(
    connection: &mut SqliteConnection,
    romfiles: &[Romfile],
    index_of: F,
) -> Option<ore_rs::Savings>
where
    F: Fn(PathBuf) -> Fut,
    Fut: Future<Output = Option<Arc<ore_rs::Index>>>,
{
    let rom_directory = get_rom_directory(connection).await;
    let paths: Vec<PathBuf> = romfiles
        .iter()
        .map(|romfile| rom_directory.join(&romfile.path))
        .collect();
    // the thin OREs, grouped by the store holding their chunks: what is
    // not an ORE, and what no store lies above, saves and pays nothing
    let ores = spawn_blocking(move || {
        let mut ores: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
        for path in paths {
            if path.extension().and_then(|extension| extension.to_str()) != Some(ORE_EXTENSION) {
                continue;
            }
            if let Some(store) = ore_rs::find_store(&path) {
                ores.entry(store).or_default().push(path);
            }
        }
        ores
    })
    .await
    .ok()?;
    let mut savings = ore_rs::Savings {
        dedup_saved: 0,
        parity: 0,
    };
    let mut some_store = false;
    for (store, paths) in ores {
        // a store that cannot be read neither pays nor is paid for
        let Some(index) = index_of(store).await else {
            continue;
        };
        some_store = true;
        let games = spawn_blocking(move || {
            let paths: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
            index.savings(&paths)
        })
        .await
        .ok()?;
        for game in games.into_iter().flatten() {
            savings.dedup_saved += game.dedup_saved;
            savings.parity += game.parity;
        }
    }
    some_store.then_some(savings)
}

/// What `romfiles` weigh on disk: their files, with a thin ORE's share of
/// its store and a WUA's of the update partitions it keeps in its own. A
/// store shared with other systems, as `GROUP_SUBSYSTEMS` gives, is split
/// by game, so each system counts only its part. Reads every thin ORE's
/// header and its store's index, and with WUAs every WUA's layout.
#[cfg(any(feature = "mcp", test))]
pub async fn actual_size(connection: &mut SqliteConnection, romfiles: &[Romfile]) -> Result<u64> {
    actual_size_in(connection, romfiles, |store| async move {
        spawn_blocking(move || ore_rs::Index::load(&store).ok().map(Arc::new))
            .await
            .ok()
            .flatten()
    })
    .await
}

/// The same weight, with the system's store index already in hand — asked
/// of the request's loader by the field before it weighed anything. The
/// preload answers its own store; a thin ORE beneath a different one reads
/// that index itself, as a query reading several sizes of one system parses
/// each index once, whatever the number of fields that need it.
#[cfg(feature = "server")]
pub async fn actual_size_loaded(
    connection: &mut SqliteConnection,
    romfiles: &[Romfile],
    store: Option<Arc<StoreIndex>>,
) -> Result<u64> {
    actual_size_in(connection, romfiles, |path| async {
        match &store {
            Some(store) => store.index_for(&path).await,
            None => spawn_blocking(move || ore_rs::Index::load(&path).ok().map(Arc::new))
                .await
                .ok()
                .flatten(),
        }
    })
    .await
}

/// The weight of `romfiles`, with every store's index obtained through
/// `index_of` — read off disk by [`actual_size`], handed over by the
/// request's loader by [`actual_size_loaded`] — so a caller that reads
/// the same store many times parses it once.
#[cfg(any(feature = "mcp", test))]
async fn actual_size_in<F, Fut>(
    connection: &mut SqliteConnection,
    romfiles: &[Romfile],
    index_of: F,
) -> Result<u64>
where
    F: Fn(PathBuf) -> Fut,
    Fut: Future<Output = Option<Arc<ore_rs::Index>>>,
{
    let rom_directory = get_rom_directory(connection).await;
    let is_wua = |path: &Path| {
        path.extension().and_then(|extension| extension.to_str()) == Some(WUA_EXTENSION)
    };
    let romfiles: Vec<(PathBuf, u64)> = romfiles
        .iter()
        .map(|romfile| (rom_directory.join(&romfile.path), romfile.size as u64))
        .collect();
    // every WUA the database knows, as they all split the partitions they share
    let all_wuas: Vec<PathBuf> = match romfiles.iter().any(|(path, _)| is_wua(path)) {
        true => crate::database::find_romfiles(connection)
            .await
            .into_iter()
            .map(|romfile| rom_directory.join(&romfile.path))
            .filter(|path| is_wua(path))
            .collect(),
        false => vec![],
    };
    // what weighs alone, what shares a store, what keeps its own: the
    // thin OREs grouped by the store holding their chunks
    let (alone, ores, wuas) = spawn_blocking(move || {
        let mut alone = 0;
        let mut ores: HashMap<PathBuf, Vec<(PathBuf, u64)>> = HashMap::new();
        let mut wuas = vec![];
        for (path, file_size) in romfiles {
            let extension = path.extension().and_then(|extension| extension.to_str());
            match (extension, ore_rs::find_store(&path)) {
                (Some(ORE_EXTENSION), Some(store)) => {
                    ores.entry(store).or_default().push((path, file_size));
                    continue;
                }
                (Some(WUA_EXTENSION), _) => wuas.push(path),
                _ => {}
            }
            alone += file_size;
        }
        (alone, ores, wuas)
    })
    .await?;
    // one index per store, however the caller chose to obtain it; a store
    // that cannot be read keeps none, as it counts as its files
    let mut indexes: HashMap<PathBuf, Arc<ore_rs::Index>> = HashMap::new();
    for store in ores.keys() {
        if let Some(index) = index_of(store.clone()).await {
            indexes.insert(store.clone(), index);
        }
    }
    Ok(spawn_blocking(move || {
        let mut size = alone;
        for (store, ores) in ores {
            // an unreadable store or ORE counts as its files, this is a size, not a check
            let paths: Vec<&Path> = ores.iter().map(|(path, _)| path.as_path()).collect();
            let shares = indexes
                .get(&store)
                .map(|index| index.shares(&paths))
                .unwrap_or_default();
            for (i, (_, file_size)) in ores.iter().enumerate() {
                size += shares.get(i).copied().flatten().unwrap_or(*file_size);
            }
        }
        if !wuas.is_empty() {
            size += wua_store_share(&all_wuas, &wuas);
        }
        size
    })
    .await?)
}

/// The share of `wuas` in their update partition stores: each entry split
/// among every WUA of `all_wuas` that needs it.
#[cfg(any(feature = "mcp", test))]
fn wua_store_share(all_wuas: &[PathBuf], wuas: &[PathBuf]) -> u64 {
    let entries = |wua: &Path| -> Vec<PathBuf> {
        let Some(store) = crate::wux::find_store(wua) else {
            return vec![];
        };
        wux_rs::store_entries(wua)
            .unwrap_or_default()
            .into_iter()
            .map(|name| store.join(name))
            .collect()
    };
    let mut entries_by_wua: HashMap<&Path, Vec<PathBuf>> = HashMap::new();
    for wua in all_wuas.iter().chain(wuas) {
        entries_by_wua
            .entry(wua.as_path())
            .or_insert_with(|| entries(wua));
    }
    let mut refs: HashMap<&Path, u64> = HashMap::new();
    for wua in all_wuas {
        for entry in &entries_by_wua[wua.as_path()] {
            *refs.entry(entry.as_path()).or_default() += 1;
        }
    }
    let mut share = 0;
    for wua in wuas {
        for entry in &entries_by_wua[wua.as_path()] {
            let len = entry.metadata().map_or(0, |metadata| metadata.len());
            share += len / refs.get(entry.as_path()).copied().unwrap_or(1).max(1);
        }
    }
    share
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
    let settings_id = store_settings_system_id(connection, system).await?;
    let store_on = get_bool(connection, "ORE_STORE", Some(settings_id)).await;
    let per_game = OreStoreBatch::from_str(
        &get_string(connection, "ORE_STORE_BATCH", Some(settings_id))
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
            moving.push((
                path,
                Options {
                    profile,
                    codec,
                    ..Options::default()
                },
            ));
        }
    }
    if moving.is_empty() {
        return Ok(());
    }

    let store = system_store(connection, system).await?;
    if store_on {
        prepare_store(&store, store_parity(connection, settings_id).await)
            .with_context(|| format!("Failed to set up \"{}\"", store.display()))?;
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
                batch.take().unwrap().commit(&mut |_| {})?;
            }
        }
        if let Some(batch) = batch {
            batch.commit(&mut |_| {})?;
        }
        // The run before this one may have taken OREs out of the store and
        // back in, a standalone written over the path a thin one had: the
        // copy it replaced keeps references no batch can give back. One
        // recount makes the sizes true from now, not from the next check.
        // The run before this one may have taken OREs out of the store and
        // back in, a standalone written over the path a thin one had: the
        // copy it replaced keeps references no batch can give back. One
        // recount makes the sizes true from now, not from the next check.
        ore_rs::recount(&store)?;
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
    let size = ore_rs::store_size(store)?;
    let checked = run_blocking(
        progress_bar,
        size,
        move |progress| -> ore_rs::Result<ore_rs::StoreChecked> {
            let checked = ore_rs::check_store(&path, true, progress)?;
            // Whole in its bytes, the store is made whole in its counts too:
            // a game purged, detached or rewritten since the last compaction
            // leaves `refs` behind, and every size the store reports divides
            // by them. A store with groups past repair is not recounted: its
            // unreadable games would fail the count for a worse message.
            if checked.lost_groups == 0 {
                ore_rs::recount(&path)?;
            }
            Ok(checked)
        },
    )
    .await
    .with_context(|| format!("Failed to check \"{}\"", store.display()))?;
    stop_action(progress_bar);
    // The store is whole: what its dedup saved and what its parity costs,
    // the numbers its size alone cannot tell
    if let Ok(stats) = ore_rs::store_stats(store) {
        print_info(
            progress_bar,
            &format!(
                "{} chunk(s): dedup saved {}, parity costs {}",
                stats.chunks,
                HumanBytes(stats.dedup_saved),
                HumanBytes(stats.parity),
            ),
        );
    }
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
/// pack small groups together, and seal again with `parity` the segments
/// sealed with other parity; with `recompress`, pack every group again.
pub async fn compact_store(
    progress_bar: &ProgressBar,
    store: &Path,
    profile: &OreProfile,
    parity: u8,
    recompress: bool,
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
    let size = ore_rs::store_size(store)?;
    let compacted = run_blocking(progress_bar, size, move |progress| {
        ore_rs::set_parity(&path, parity)?;
        ore_rs::compact(&path, &options, 0.5, recompress, progress)
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

/// The Wii and Wii U keys, where `import-keys` stores them, as ore-rs reads
/// them; in tests, the made-up keys of the synthetic Wii U disc.
fn load_wii_keys() -> Result<Vec<ore_rs::Key>> {
    let path = super::wux::keys_path()?;
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

#[cfg(test)]
mod tests;

//! Wii U discs: WUD, the raw dump, and WUX, its lossless compression, both
//! handled by the wux-rs crate, which also exports either to Cemu's WUA.
//!
//! WUA needs the disc's key and the Wii U common key. Neither ships with
//! oxyromon: `import-keys` stores them in its data directory, from files in
//! the format of Cemu's `keys.txt`, where the common key can sit alongside the
//! disc keys. Like Cemu, wux-rs tries each key.

use super::common::*;
use super::config::*;
use super::database::{find_romfiles_of_tmds, find_roms_by_game_id_no_parents};
use super::import_keys::WIIU_KEYS_FILE;
use super::mimetype::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use anyhow::{Context, Result, bail};
use indicatif::ProgressBar;
use sqlx::SqliteConnection;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::task::spawn_blocking;

/// The keys `import-keys` stored; in tests, the made-up keys of the synthetic
/// disc.
fn keys_path() -> Result<PathBuf> {
    if cfg!(test) {
        return Ok(PathBuf::from("tests/Test Game (USA).keys.txt"));
    }
    Ok(get_data_directory()?.join(WIIU_KEYS_FILE))
}

pub struct WudRomfile {
    pub romfile: CommonRomfile,
}

impl GetRomfile for WudRomfile {
    fn romfile(&self) -> &CommonRomfile {
        &self.romfile
    }
}

pub struct WuxRomfile {
    pub romfile: CommonRomfile,
}

impl GetRomfile for WuxRomfile {
    fn romfile(&self) -> &CommonRomfile {
        &self.romfile
    }
}

impl HashAndSize for WuxRomfile {
    async fn get_hash_and_size(
        &self,
        _connection: &mut SqliteConnection,
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

        // Hashed as it decompresses: a WUD is 25 GB, not worth writing out
        let path = self.romfile.path.clone();
        let hash_algorithm = *hash_algorithm;
        let bar = progress_bar.clone();
        let (hash, size) = spawn_blocking(move || {
            let mut image = wux_rs::Image::open(&path)?;
            bar.set_style(get_bytes_progress_style());
            bar.set_length(image.size());
            hash_reader(&mut image, &bar, &hash_algorithm)
        })
        .await
        .context("WUX reader task failed")??;

        progress_bar.set_message("");

        Ok((hash, size))
    }
}

impl Check for WuxRomfile {
    async fn check(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        roms: &[&Rom],
    ) -> Result<()> {
        print_action(progress_bar, &format!("Checking \"{}\"", self.romfile));
        // No Wii U DAT declares a header, but honour one the long way round
        if header.is_none() {
            let rom = roms[0];
            let hash_algorithm = get_hash_algorithm(rom)?;
            let (hash, size) = self
                .get_hash_and_size(connection, progress_bar, 1, 1, &hash_algorithm)
                .await?;
            return compare_hash_and_size(rom, &hash, size, &hash_algorithm);
        }
        let tmp_directory = create_tmp_directory(connection).await?;
        let wud_romfile = self.to_wud(progress_bar, &tmp_directory.path()).await?;
        wud_romfile
            .romfile
            .check(connection, progress_bar, header, roms)
            .await
    }
}

pub trait ToWud {
    async fn to_wud<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<WudRomfile>;
}

impl ToWud for WuxRomfile {
    async fn to_wud<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<WudRomfile> {
        start_action(progress_bar, Some("Extracting wux"));

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
            .with_extension(WUD_EXTENSION);
        let (input, output) = (self.romfile.path.clone(), path.clone());
        run_blocking(progress_bar, input.metadata()?.len(), move |progress| {
            wux_rs::decompress(&input, &output, progress)
        })
        .await
        .with_context(|| format!("Failed to extract \"{}\"", self.romfile.path.display()))?;

        stop_action(progress_bar);

        CommonRomfile::from_path(&path)?.as_wud()
    }
}

pub trait ToWux {
    async fn to_wux<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<WuxRomfile>;
}

impl ToWux for WudRomfile {
    async fn to_wux<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<WuxRomfile> {
        start_action(progress_bar, Some("Creating wux"));

        let path = destination_directory
            .as_ref()
            .join(self.romfile.path.file_name().unwrap())
            .with_extension(WUX_EXTENSION);

        print_action(
            progress_bar,
            &format!(
                "Creating \"{}\"",
                path.file_name().unwrap().to_str().unwrap()
            ),
        );

        let (input, output) = (self.romfile.path.clone(), path.clone());
        run_blocking(progress_bar, input.metadata()?.len(), move |progress| {
            wux_rs::compress(&input, &output, progress)
        })
        .await
        .with_context(|| format!("Failed to create \"{}\"", path.display()))?;

        stop_action(progress_bar);

        CommonRomfile::from_path(&path)?.as_wux()
    }
}

/// Export to WUA, which drops the disc's encryption and layout: there is no
/// way back, so a WUA is never imported or converted from.
pub trait ToWua: GetRomfile {
    /// The WUA holds the game and, as Cemu bundles them, the newest update and
    /// DLC found in the library, in any system, matched by title ID.
    async fn to_wua<P: AsRef<Path>>(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<CommonRomfile> {
        start_action(progress_bar, Some("Creating wua"));

        let input = self.romfile().path.clone();
        let path = destination_directory
            .as_ref()
            .join(input.file_name().unwrap())
            .with_extension(WUA_EXTENSION);

        print_action(
            progress_bar,
            &format!(
                "Creating \"{}\"",
                path.file_name().unwrap().to_str().unwrap()
            ),
        );

        let keys = load_wiiu_keys()?;
        let mut inputs = vec![input.clone()];
        inputs.extend(find_update_and_dlc(connection, progress_bar, &input, &keys).await?);
        let mut length = 0;
        for input in &inputs {
            length += match input.is_dir() {
                true => directory_size(input)?,
                false => input.metadata()?.len(),
            };
        }
        let output = path.clone();
        run_blocking(progress_bar, length, move |progress| {
            let inputs: Vec<&Path> = inputs.iter().map(PathBuf::as_path).collect();
            wux_rs::to_wua(&inputs, &output, &keys, progress)
        })
        .await
        .with_context(|| format!("Failed to create \"{}\"", path.display()))?;

        stop_action(progress_bar);

        CommonRomfile::from_path(&path)
    }
}

/// The folders of the newest update and DLC in the library for the game on
/// the disc at `disc`: titles whose ID shares the game's low half, typed
/// `0005000E` for an update and `0005000C` for DLC.
async fn find_update_and_dlc(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    disc: &Path,
    keys: &[wux_rs::Key],
) -> Result<Vec<PathBuf>> {
    let tmds = find_romfiles_of_tmds(connection).await;
    if tmds.is_empty() {
        return Ok(Vec::new());
    }
    let (title_id, _) = wux_rs::title_id(disc, keys)
        .with_context(|| format!("Failed to read \"{}\"", disc.display()))?;
    // the newest version of each matching title
    let mut found: HashMap<u64, (u16, PathBuf)> = HashMap::new();
    for tmd in tmds {
        let path = tmd.as_common(connection).await?.path;
        let Some(directory) = path.parent() else {
            continue;
        };
        let Ok((id, version)) = wux_rs::title_id(directory, &[]) else {
            continue;
        };
        if id & 0xFFFF_FFFF != title_id & 0xFFFF_FFFF
            || !matches!(id >> 32, 0x0005_000E | 0x0005_000C)
        {
            continue;
        }
        // No-Intro lists DLC without the ticket it would need
        if wux_rs::ticket_path(directory).is_none() {
            print_warning(
                progress_bar,
                &format!(
                    "\"{}\" has no ticket to decrypt it with, skipping",
                    directory.file_name().unwrap().to_str().unwrap()
                ),
            );
            continue;
        }
        if found.get(&id).is_none_or(|(newest, _)| version > *newest) {
            found.insert(id, (version, directory.to_path_buf()));
        }
    }
    let mut directories: Vec<PathBuf> = found.into_values().map(|(_, path)| path).collect();
    directories.sort();
    for directory in &directories {
        print_info(
            progress_bar,
            &format!(
                "Adding \"{}\"",
                directory.file_name().unwrap().to_str().unwrap()
            ),
        );
    }
    Ok(directories)
}

/// The total size of the files directly in `directory`.
fn directory_size(directory: &Path) -> Result<u64> {
    let mut size = 0;
    for entry in std::fs::read_dir(directory)? {
        let metadata = entry?.metadata()?;
        if metadata.is_file() {
            size += metadata.len();
        }
    }
    Ok(size)
}

/// Export to a NUS folder, the form Nintendo's servers deliver titles in and
/// WUP Installer GX2 installs: the disc's TMD, ticket and certificates, and
/// its contents, which are the CDN's byte for byte, checked against the TMD.
pub trait ToNus: GetRomfile {
    async fn to_nus<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<PathBuf> {
        start_action(progress_bar, Some("Creating nus"));

        let input = self.romfile().path.clone();
        let path = destination_directory
            .as_ref()
            .join(input.file_stem().unwrap());

        print_action(
            progress_bar,
            &format!(
                "Creating \"{}\"",
                path.file_name().unwrap().to_str().unwrap()
            ),
        );

        let keys = load_wiiu_keys()?;
        let output = path.clone();
        run_blocking(progress_bar, input.metadata()?.len(), move |progress| {
            wux_rs::to_nus(&input, &output, &keys, progress)
        })
        .await
        .with_context(|| format!("Failed to create \"{}\"", path.display()))?;

        stop_action(progress_bar);

        Ok(path)
    }
}

impl ToNus for WudRomfile {}

impl ToNus for WuxRomfile {}

/// The keys `import-keys` stored.
fn load_wiiu_keys() -> Result<Vec<wux_rs::Key>> {
    let keys_path = keys_path()?;
    if !keys_path.is_file() {
        bail!("No Wii U keys imported: run import-keys with a keys.txt such as Cemu's");
    }
    wux_rs::load_keys(&keys_path)
        .with_context(|| format!("Failed to read \"{}\"", keys_path.display()))
}

impl ToWua for WudRomfile {}

impl ToWua for WuxRomfile {}

pub trait AsWud {
    fn as_wud(self) -> Result<WudRomfile>;
}

impl AsWud for CommonRomfile {
    fn as_wud(self) -> Result<WudRomfile> {
        if !has_extension(&self.path, WUD_EXTENSION) {
            bail!("Not a valid wud");
        }
        Ok(WudRomfile { romfile: self })
    }
}

pub trait AsWux {
    fn as_wux(self) -> Result<WuxRomfile>;
}

impl AsWux for CommonRomfile {
    fn as_wux(self) -> Result<WuxRomfile> {
        if !has_extension(&self.path, WUX_EXTENSION) {
            bail!("Not a valid wux");
        }
        Ok(WuxRomfile { romfile: self })
    }
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(extension))
}

/// Whether a ROM is a NUS title's TMD, named by a downloader (`title.tmd`)
/// or as No-Intro keeps the CDN's names (`tmd`, `tmd.<version>`).
pub fn is_tmd_name(name: &str) -> bool {
    name == "title.tmd"
        || name.strip_prefix("tmd").is_some_and(|rest| {
            rest.is_empty()
                || rest
                    .strip_prefix('.')
                    .is_some_and(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
        })
}

/// Whether `game` is a NUS title, an update or DLC as Nintendo's servers
/// deliver it. Every such title names its files alike, `00000000` and on, so
/// each keeps them in a folder of its own.
pub async fn is_nus_title(connection: &mut SqliteConnection, game: &Game) -> bool {
    find_roms_by_game_id_no_parents(connection, game.id)
        .await
        .iter()
        .any(|rom| is_tmd_name(&rom.name))
}

/// Reported by `info`. It never fails, since only WUA needs keys, but says
/// when no Wii U key was imported so that isn't first discovered mid-export.
pub async fn get_version() -> Result<String> {
    Ok(match keys_path().is_ok_and(|path| path.is_file()) {
        true => String::from("built-in"),
        false => String::from("built-in, no Wii U keys imported"),
    })
}

//! Wii U discs: WUD, the raw dump, which Redump lists as an ISO; WUX, its
//! lossless compression; and WUA, Cemu's archive of a disc's files, lossless
//! too when wux-rs writes it, as it then keeps what rebuilds the disc byte
//! for byte. All are handled by the wux-rs crate.
//!
//! A disc's update partition, the same on every disc carrying that update,
//! is kept once for every WUA of a system, in a store: a hidden folder in the
//! system's directory. A WUA finds it in the nearest folder up from its own.
//!
//! WUA and NUS need the disc's key and the Wii U common key. Neither ships
//! with oxyromon: `import-keys` stores them in its data directory, from files
//! in the format of Cemu's `keys.txt`, where the common key can sit alongside
//! the disc keys. Like Cemu, wux-rs tries each key.
//!
//! A title needs its ticket too. No-Intro lists none for DLC, which
//! Nintendo's servers deliver without one, so the ticket a downloader made
//! is kept apart, in a store of tickets in the data directory, by title ID.

use super::common::*;
use super::config::*;
use super::database::{
    find_multi_version_nus_game_ids_by_system_id, find_romfile_by_id, find_romfiles_of_tmds,
    find_roms_by_game_id_no_parents, update_game_completion,
};
use super::import_keys::WIIU_KEYS_FILE;
use super::mimetype::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use anyhow::{Context, Result, bail};
use indicatif::ProgressBar;
use sqlx::SqliteConnection;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tokio::task::spawn_blocking;

pub use wux_rs::STORE_DIR;

/// The keys `import-keys` stored; in tests, the made-up keys of the synthetic
/// disc.
fn keys_path() -> Result<PathBuf> {
    if cfg!(test) {
        return Ok(PathBuf::from("tests/Test Game (USA).keys.txt"));
    }
    Ok(get_data_directory()?.join(WIIU_KEYS_FILE))
}

/// The store of the tickets No-Intro does not list, a `<title id>.tik` each.
const WIIU_TICKETS_DIRECTORY: &str = "wiiu_tickets";

/// Where the tickets No-Intro does not list are kept: in the data directory,
/// beside the keys, as a title ID names one title whatever its system; in
/// tests, in the temp directory, which each test sets.
async fn tickets_directory(connection: &mut SqliteConnection) -> Result<PathBuf> {
    if cfg!(test) {
        return Ok(get_tmp_directory(connection)
            .await
            .join(WIIU_TICKETS_DIRECTORY));
    }
    Ok(get_data_directory()?.join(WIIU_TICKETS_DIRECTORY))
}

/// The ticket kept for `title_id`, if any.
pub async fn stored_ticket(
    connection: &mut SqliteConnection,
    title_id: u64,
) -> Result<Option<PathBuf>> {
    let path = tickets_directory(connection)
        .await?
        .join(format!("{title_id:016x}.tik"));
    Ok(path.is_file().then_some(path))
}

/// Move the ticket at `path`, for `title_id`, to the store of tickets.
pub async fn store_ticket(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    path: &Path,
    title_id: u64,
) -> Result<()> {
    let directory = tickets_directory(connection).await?;
    create_directory(progress_bar, &directory, true).await?;
    rename_file(
        progress_bar,
        &path,
        &directory.join(format!("{title_id:016x}.tik")),
        true,
    )
    .await?;
    print_success(
        progress_bar,
        &format!("Kept the ticket of {title_id:016x}, which the DAT does not list"),
    );
    Ok(())
}

/// Give the NUS title exported to `directory` the ticket kept for it, as
/// `title.tik`, when it has none of its own.
pub async fn add_stored_ticket(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    directory: &Path,
) -> Result<()> {
    if wux_rs::ticket_path(directory).is_some() {
        return Ok(());
    }
    let Ok((title_id, _)) = wux_rs::title_id(directory, &[]) else {
        return Ok(());
    };
    if let Some(path) = stored_ticket(connection, title_id).await? {
        copy_file(progress_bar, &path, &directory.join("title.tik"), true).await?;
    }
    Ok(())
}

/// The store a system's WUAs share their update partitions in.
pub async fn store_directory(
    connection: &mut SqliteConnection,
    system: &System,
) -> Result<PathBuf> {
    Ok(get_system_directory(connection, system)
        .await?
        .join(STORE_DIR))
}

/// The store of the WUA at `path`: the nearest one up from its folder.
pub fn find_store(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .skip(1)
        .map(|dir| dir.join(STORE_DIR))
        .find(|store| store.is_dir())
}

/// Copy the store entries the WUA at `wua` needs from `from` to `to`, those
/// `to` lacks. Returns the names of those `from` lacks too.
pub async fn copy_store_entries(
    progress_bar: &ProgressBar,
    wua: &Path,
    from: Option<&Path>,
    to: &Path,
) -> Result<Vec<String>> {
    let mut missing = Vec::new();
    for name in wux_rs::store_entries(wua)? {
        let target = to.join(&name);
        if target.is_file() {
            continue;
        }
        match from
            .map(|from| from.join(&name))
            .filter(|path| path.is_file())
        {
            Some(source) => {
                create_directory(progress_bar, &to, true).await?;
                copy_file(progress_bar, &source, &target, false).await?;
            }
            None => missing.push(name),
        }
    }
    Ok(missing)
}

pub struct WudRomfile {
    pub romfile: CommonRomfile,
}

impl GetRomfile for WudRomfile {
    fn romfile(&self) -> &CommonRomfile {
        &self.romfile
    }
}

impl Check for WudRomfile {
    async fn check(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        roms: &[&Rom],
    ) -> Result<()> {
        self.romfile
            .check(connection, progress_bar, header, roms)
            .await
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
        let wud_romfile = self
            .to_wud(progress_bar, &tmp_directory.path().join(&roms[0].name))
            .await?;
        wud_romfile
            .romfile
            .check(connection, progress_bar, header, roms)
            .await
    }
}

/// A WUA wux-rs wrote with what rebuilds its disc.
pub struct WuaRomfile {
    pub romfile: CommonRomfile,
    pub info: wux_rs::DiscInfo,
}

impl GetRomfile for WuaRomfile {
    fn romfile(&self) -> &CommonRomfile {
        &self.romfile
    }
}

impl WuaRomfile {
    fn store(&self) -> Option<PathBuf> {
        find_store(&self.romfile.path)
    }

    /// The disc's hash, as a DAT lists it.
    fn hash(&self, hash_algorithm: &HashAlgorithm) -> String {
        match hash_algorithm {
            HashAlgorithm::Crc => format!("{:08x}", self.info.crc32),
            HashAlgorithm::Md5 => to_hex(&self.info.md5),
            HashAlgorithm::Sha1 => to_hex(&self.info.sha1),
        }
    }

    /// Rebuild the disc into `path`, as a WUX if `wux`, a WUD otherwise.
    async fn rebuild(&self, progress_bar: &ProgressBar, path: &Path, wux: bool) -> Result<()> {
        let keys = load_wiiu_keys()?;
        let (input, output, store) = (self.romfile.path.clone(), path.to_path_buf(), self.store());
        run_blocking(
            progress_bar,
            input.metadata()?.len(),
            move |progress| match wux {
                true => wux_rs::wua_to_wux(&input, &output, store.as_deref(), &keys, progress),
                false => wux_rs::wua_to_wud(&input, &output, store.as_deref(), &keys, progress),
            },
        )
        .await
        .with_context(|| format!("Failed to rebuild \"{}\"", self.romfile.path.display()))?;
        Ok(())
    }
}

impl HashAndSize for WuaRomfile {
    /// What the WUA says of its disc: it is checked against it when rebuilt.
    async fn get_hash_and_size(
        &self,
        _connection: &mut SqliteConnection,
        _progress_bar: &ProgressBar,
        _position: usize,
        _total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> Result<(String, u64)> {
        Ok((self.hash(hash_algorithm), self.info.size))
    }
}

impl Check for WuaRomfile {
    /// The disc the WUA rebuilds, as the DAT lists it, and rebuilt, which
    /// checks it against the SHA-1 the WUA keeps.
    async fn check(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        _header: &Option<Header>,
        roms: &[&Rom],
    ) -> Result<()> {
        print_action(progress_bar, &format!("Checking \"{}\"", self.romfile));
        let rom = roms[0];
        let hash_algorithm = get_hash_algorithm(rom)?;
        compare_hash_and_size(
            rom,
            &self.hash(&hash_algorithm),
            self.info.size,
            &hash_algorithm,
        )?;
        let tmp_directory = create_tmp_directory(connection).await?;
        start_action(progress_bar, Some("Rebuilding wux"));
        let result = self
            .rebuild(progress_bar, &tmp_directory.path().join("check.wux"), true)
            .await;
        stop_action(progress_bar);
        result
    }
}

pub trait ToWud {
    /// The disc as a WUD at `path`, named as its DAT names it.
    async fn to_wud(&self, progress_bar: &ProgressBar, path: &Path) -> Result<WudRomfile>;
}

impl ToWud for WuxRomfile {
    async fn to_wud(&self, progress_bar: &ProgressBar, path: &Path) -> Result<WudRomfile> {
        start_action(progress_bar, Some("Extracting wux"));
        print_action(
            progress_bar,
            &format!(
                "Extracting \"{}\"",
                self.romfile.path.file_name().unwrap().to_str().unwrap()
            ),
        );
        let (input, output) = (self.romfile.path.clone(), path.to_path_buf());
        run_blocking(progress_bar, input.metadata()?.len(), move |progress| {
            wux_rs::decompress(&input, &output, progress)
        })
        .await
        .with_context(|| format!("Failed to extract \"{}\"", self.romfile.path.display()))?;
        stop_action(progress_bar);
        Ok(WudRomfile {
            romfile: CommonRomfile::from_path(&path)?,
        })
    }
}

impl ToWud for WuaRomfile {
    async fn to_wud(&self, progress_bar: &ProgressBar, path: &Path) -> Result<WudRomfile> {
        start_action(progress_bar, Some("Rebuilding wud"));
        print_action(
            progress_bar,
            &format!(
                "Rebuilding \"{}\"",
                path.file_name().unwrap().to_str().unwrap()
            ),
        );
        self.rebuild(progress_bar, path, false).await?;
        stop_action(progress_bar);
        Ok(WudRomfile {
            romfile: CommonRomfile::from_path(&path)?,
        })
    }
}

/// A WUX, or a lossless WUA, as the WUD at `path`.
pub async fn disc_to_wud(
    progress_bar: &ProgressBar,
    romfile: CommonRomfile,
    path: &Path,
) -> Result<WudRomfile> {
    match has_extension(&romfile.path, WUA_EXTENSION) {
        true => romfile.as_wua()?.to_wud(progress_bar, path).await,
        false => romfile.as_wux()?.to_wud(progress_bar, path).await,
    }
}

pub trait ToWux {
    async fn to_wux<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<WuxRomfile>;
}

/// Where a disc goes in `directory` as `extension`, named after `path`.
fn output_path<P: AsRef<Path>>(directory: &P, path: &Path, extension: &str) -> PathBuf {
    directory
        .as_ref()
        .join(path.file_name().unwrap())
        .with_extension(extension)
}

impl ToWux for WudRomfile {
    async fn to_wux<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<WuxRomfile> {
        start_action(progress_bar, Some("Creating wux"));
        let path = output_path(destination_directory, &self.romfile.path, WUX_EXTENSION);
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

impl ToWux for WuaRomfile {
    async fn to_wux<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<WuxRomfile> {
        start_action(progress_bar, Some("Rebuilding wux"));
        let path = output_path(destination_directory, &self.romfile.path, WUX_EXTENSION);
        print_action(
            progress_bar,
            &format!(
                "Rebuilding \"{}\"",
                path.file_name().unwrap().to_str().unwrap()
            ),
        );
        self.rebuild(progress_bar, &path, true).await?;
        stop_action(progress_bar);
        CommonRomfile::from_path(&path)?.as_wux()
    }
}

/// Convert to WUA, which keeps what rebuilds the disc: its update partition
/// goes to `store`, shared by the WUAs beside it.
pub trait ToWua: GetRomfile {
    /// With `bundle`, the WUA holds, as Cemu bundles them, the newest update
    /// and DLC found in the library, in any system, matched by title ID.
    async fn to_wua<P: AsRef<Path>>(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        store: &Path,
        bundle: bool,
    ) -> Result<WuaRomfile> {
        start_action(progress_bar, Some("Creating wua"));
        let input = self.romfile().path.clone();
        let path = output_path(destination_directory, &input, WUA_EXTENSION);
        print_action(
            progress_bar,
            &format!(
                "Creating \"{}\"",
                path.file_name().unwrap().to_str().unwrap()
            ),
        );
        let keys = load_wiiu_keys()?;
        let mut inputs = vec![input.clone()];
        let mut tickets = Vec::new();
        if bundle {
            let (directories, stored) =
                find_update_and_dlc(connection, progress_bar, &input, &keys).await?;
            inputs.extend(directories);
            tickets = stored;
        }
        let mut length = 0;
        for input in &inputs {
            length += match input.is_dir() {
                true => directory_size(input)?,
                false => input.metadata()?.len(),
            };
        }
        let (output, store) = (path.clone(), store.to_path_buf());
        run_blocking(progress_bar, length, move |progress| {
            let inputs: Vec<&Path> = inputs.iter().map(PathBuf::as_path).collect();
            wux_rs::to_wua_with_tickets(&inputs, &output, &keys, &tickets, Some(&store), progress)
        })
        .await
        .with_context(|| format!("Failed to create \"{}\"", path.display()))?;
        stop_action(progress_bar);
        CommonRomfile::from_path(&path)?.as_wua()
    }
}

impl ToWua for WudRomfile {}

impl ToWua for WuxRomfile {}

/// The folders of the newest update and DLC in the library for the game on
/// the disc at `disc`: titles whose ID shares the game's low half, typed
/// `0005000E` for an update and `0005000C` for DLC, and the stored tickets of
/// those that hold none.
async fn find_update_and_dlc(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    disc: &Path,
    keys: &[wux_rs::Key],
) -> Result<(Vec<PathBuf>, Vec<Vec<u8>>)> {
    let tmds = find_romfiles_of_tmds(connection).await;
    if tmds.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let (title_id, _) = wux_rs::title_id(disc, keys)
        .with_context(|| format!("Failed to read \"{}\"", disc.display()))?;
    // the newest version of each matching title
    let mut found: HashMap<u64, (u16, PathBuf)> = HashMap::new();
    let mut tickets: HashMap<u64, Vec<u8>> = HashMap::new();
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
        // No-Intro lists DLC without the ticket it would need: the one a
        // downloader made is in the store of tickets, if it was imported
        if wux_rs::ticket_path(directory).is_none() && !tickets.contains_key(&id) {
            match stored_ticket(connection, id).await? {
                Some(path) => {
                    tickets.insert(id, std::fs::read(&path)?);
                }
                None => {
                    print_warning(
                        progress_bar,
                        &format!(
                            "\"{}\" has no ticket to decrypt it with, skipping: import its folder with the ticket a downloader made",
                            directory.file_name().unwrap().to_str().unwrap()
                        ),
                    );
                    continue;
                }
            }
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
    Ok((directories, tickets.into_values().collect()))
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
        let is_wua = has_extension(&input, WUA_EXTENSION);
        run_blocking(
            progress_bar,
            input.metadata()?.len(),
            move |progress| match is_wua {
                true => wua_to_disc_nus(&input, &output, &keys, progress),
                false => wux_rs::to_nus(&input, &output, &keys, progress),
            },
        )
        .await
        .with_context(|| format!("Failed to create \"{}\"", path.display()))?;
        stop_action(progress_bar);
        Ok(path)
    }
}

/// A lossless WUA's disc as a NUS folder at `output`: its revert data
/// rebuilds the game's title as converting the disc gives it.
fn wua_to_disc_nus(
    input: &Path,
    output: &Path,
    keys: &[wux_rs::Key],
    progress: &mut dyn FnMut(u64),
) -> wux_rs::Result<wux_rs::Stats> {
    let titles = output.with_extension("titles");
    let stats = wux_rs::wua_to_nus(input, &titles, keys, progress)?;
    let result = (|| {
        // The game's folder: the title a disc holds is a game, 00050000.
        let game = std::fs::read_dir(&titles)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("00050000"))
            })
            .ok_or_else(|| wux_rs::Error::Corrupt("the WUA holds no game".into()))?;
        // only an earlier export of the same is replaced
        if output.exists() {
            if !holds_only(output, &game) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!(
                        "{} is in the way, and holds files this would not write",
                        output.display()
                    ),
                )
                .into());
            }
            std::fs::remove_dir_all(output)?;
        }
        std::fs::rename(game, output)?;
        Ok(stats)
    })();
    let _ = std::fs::remove_dir_all(&titles);
    result
}

/// Whether every file in `dir`, and in its folders, has its like in `like`,
/// so that replacing `dir` with `like` loses nothing.
fn holds_only(dir: &Path, like: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut entries| {
        entries.all(|entry| {
            entry.is_ok_and(|entry| {
                let other = like.join(entry.file_name());
                match entry.file_type() {
                    Ok(kind) if kind.is_dir() => {
                        other.is_dir() && holds_only(&entry.path(), &other)
                    }
                    Ok(_) => other.is_file(),
                    Err(_) => false,
                }
            })
        })
    })
}

impl ToNus for WudRomfile {}

impl ToNus for WuxRomfile {}

impl ToNus for WuaRomfile {}

/// The keys `import-keys` stored.
fn load_wiiu_keys() -> Result<Vec<wux_rs::Key>> {
    let keys_path = keys_path()?;
    if !keys_path.is_file() {
        bail!("No Wii U keys imported: run import-keys with a keys.txt such as Cemu's");
    }
    wux_rs::load_keys(&keys_path)
        .with_context(|| format!("Failed to read \"{}\"", keys_path.display()))
}

pub trait AsWud {
    fn as_wud(self) -> Result<WudRomfile>;
}

impl AsWud for CommonRomfile {
    /// A WUD, as `.wud` or as Redump's `.iso`: told from other ISOs by the
    /// disc's magic.
    fn as_wud(self) -> Result<WudRomfile> {
        if !is_wud(&self.path) {
            bail!("Not a valid wud");
        }
        Ok(WudRomfile { romfile: self })
    }
}

/// Whether `path` is a Wii U disc's WUD, named `.wud` or `.iso`.
pub fn is_wud(path: &Path) -> bool {
    (has_extension(path, WUD_EXTENSION) || has_extension(path, ISO_EXTENSION))
        && wux_rs::Image::open(path).is_ok_and(|image| {
            let mut magic = [0; 4];
            image.header().is_none()
                && image.read_exact_at(&mut magic, 0x10000).is_ok()
                && magic == [0xCC, 0x54, 0x9E, 0xB9]
        })
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

pub trait AsWua {
    fn as_wua(self) -> Result<WuaRomfile>;
}

impl AsWua for CommonRomfile {
    /// A WUA that rebuilds its disc: those Cemu writes, or wux-rs did before,
    /// can't, and are not lossless.
    fn as_wua(self) -> Result<WuaRomfile> {
        if !has_extension(&self.path, WUA_EXTENSION) {
            bail!("Not a valid wua");
        }
        let info = wux_rs::disc_info(&self.path)
            .with_context(|| format!("Failed to read \"{}\"", self.path.display()))?;
        match info {
            Some(info) => Ok(WuaRomfile {
                romfile: self,
                info,
            }),
            None => bail!("Not a lossless wua: it can't rebuild its disc"),
        }
    }
}

pub fn has_extension(path: &Path, extension: &str) -> bool {
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

/// Recompute the completion of a system's NUS titles whose DAT entry spans
/// several versions, as No-Intro's CDN DAT lists them: each version's TMD, and
/// every content any of them used. Nintendo's servers deliver the newest
/// version, whose TMD lists only the contents it uses, so such a title is
/// complete when its newest TMD and the files that TMD lists are; older TMDs,
/// and contents only older versions use, are ignored. A title whose newest TMD
/// is missing keeps the completion computed from all its ROMs.
pub async fn update_nus_games_completion(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system_id: i64,
) -> Result<()> {
    for game_id in find_multi_version_nus_game_ids_by_system_id(connection, system_id).await {
        let roms = find_roms_by_game_id_no_parents(connection, game_id).await;
        let Some(newest) = roms
            .iter()
            .filter_map(|rom| Some((rom.name.strip_prefix("tmd.")?.parse::<u16>().ok()?, rom)))
            .max_by_key(|(version, _)| *version)
            .map(|(_, rom)| rom)
        else {
            continue;
        };
        let Some(romfile_id) = newest.romfile_id else {
            continue;
        };
        let path = find_romfile_by_id(connection, romfile_id)
            .await
            .as_common(connection)
            .await?
            .path;
        let tmd = match tokio::fs::read(&path)
            .await
            .map_err(anyhow::Error::from)
            .and_then(|bytes| Ok(wux_rs::parse_tmd(&bytes)?))
        {
            Ok(tmd) => tmd,
            Err(e) => {
                print_warning(
                    progress_bar,
                    &format!("Failed to read \"{}\": {e:#}", path.display()),
                );
                continue;
            }
        };
        let contents: HashSet<String> = tmd
            .contents
            .iter()
            .map(|content| format!("{:08x}", content.id))
            .collect();
        let complete = roms
            .iter()
            .filter(|rom| {
                if is_tmd_name(&rom.name) {
                    return rom.id == newest.id;
                }
                let name = rom.name.strip_suffix(".h3").unwrap_or(&rom.name);
                !is_content_name(name) || contents.contains(name)
            })
            .all(|rom| rom.romfile_id.is_some());
        let completion = match complete {
            true => Completion::Full,
            false => Completion::Partial,
        };
        update_game_completion(connection, game_id, completion).await;
    }
    Ok(())
}

/// Write the H3 files the NUS title exported to `directory` lacks: No-Intro
/// sometimes leaves one out of its DATs, though the title installs only with
/// it. A failure only warns, since the rest of the title is exported.
pub fn write_missing_h3(progress_bar: &ProgressBar, directory: &Path) {
    // Keys are needed only if an H3 file is missing.
    let (keys, keys_error) = match load_wiiu_keys() {
        Ok(keys) => (keys, None),
        Err(error) => (Vec::new(), Some(error)),
    };
    match wux_rs::write_missing_h3(directory, &keys) {
        Ok(paths) => {
            for path in paths {
                print_info(
                    progress_bar,
                    &format!(
                        "Rebuilt \"{}\", which the DAT leaves out",
                        path.file_name().unwrap().to_str().unwrap()
                    ),
                );
            }
        }
        Err(error) => {
            let error = match (error, keys_error) {
                (wux_rs::Error::MissingKey(_), Some(keys_error)) => keys_error.to_string(),
                (error, _) => error.to_string(),
            };
            print_warning(
                progress_bar,
                &format!(
                    "Failed to rebuild the H3 files of \"{}\": {error}",
                    directory.file_name().unwrap().to_str().unwrap()
                ),
            );
        }
    }
}

/// Whether `name` is a NUS content's CDN name, its ID in eight hex digits.
fn is_content_name(name: &str) -> bool {
    name.len() == 8 && name.bytes().all(|b| b.is_ascii_hexdigit())
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

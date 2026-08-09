// === IMPORTS ===
use super::config::*;
use super::crc32::*;
use super::database::*;
use super::generate_playlists::DISC_REGEX;
use super::mimetype::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use anyhow::{Context, Result, bail};
use core::fmt;
use digest::Digest;
use digest_io::IoWrapper;
use indexmap::IndexMap;
use indicatif::ProgressBar;
use md5::Md5;
use num_traits::FromPrimitive;
use rayon::prelude::*;
use sha1::Sha1;
use sqlx::SqliteConnection;
use std::collections::HashMap;
use std::io;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::{fs::File, str::FromStr};

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Splits games into those whose romfiles match any of the given extensions and the rest.
pub fn partition_games_by_extensions(
    roms_by_game_id: IndexMap<i64, Vec<Rom>>,
    romfiles_by_id: &HashMap<i64, Romfile>,
    extensions: &[&str],
) -> (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) {
    roms_by_game_id.into_iter().partition(|(_, roms)| {
        roms.par_iter().any(|rom| {
            let path = &romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap().path;
            extensions.iter().any(|extension| path.ends_with(extension))
        })
    })
}

/// Splits games into those whose romfiles cover all of the given extensions and the rest.
pub fn partition_games_by_all_extensions(
    roms_by_game_id: IndexMap<i64, Vec<Rom>>,
    romfiles_by_id: &HashMap<i64, Romfile>,
    extensions: &[&str],
) -> (IndexMap<i64, Vec<Rom>>, IndexMap<i64, Vec<Rom>>) {
    roms_by_game_id.into_iter().partition(|(_, roms)| {
        extensions.iter().all(|extension| {
            roms.par_iter().any(|rom| {
                romfiles_by_id
                    .get(&rom.romfile_id.unwrap())
                    .unwrap()
                    .path
                    .ends_with(extension)
            })
        })
    })
}

// === CORE TYPES ===
#[derive(Clone)]
pub struct CommonRomfile {
    pub path: PathBuf,
    pub system_id: Option<i64>,
}

impl CommonRomfile {
    pub fn with_system(mut self, system_id: Option<i64>) -> Self {
        self.system_id = system_id;
        self
    }
}

pub struct IsoRomfile {
    pub romfile: CommonRomfile,
}

pub struct CueBinRomfile {
    pub cue_romfile: CommonRomfile,
    pub bin_romfiles: Vec<CommonRomfile>,
}

// === CORE TRAITS ===
pub trait FromPath<T> {
    fn from_path<P: AsRef<Path>>(path: &P) -> Result<T>;
}

pub trait CommonFile {
    async fn get_sorted_path(
        &self,
        connection: &mut SqliteConnection,
        system: &System,
        game: &Game,
        rom: &Rom,
        subfolders: &Option<SubfolderScheme>,
        extension: &Option<&str>,
    ) -> Result<PathBuf>;

    async fn get_relative_path(&self, connection: &mut SqliteConnection) -> Result<&Path>;

    async fn rename<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        new_path: &P,
        quiet: bool,
    ) -> Result<CommonRomfile>;

    async fn copy<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        quiet: bool,
    ) -> Result<CommonRomfile>;

    async fn delete(&self, progress_bar: &ProgressBar, quiet: bool) -> Result<()>;
}

/// Access to the underlying file of a format-specific romfile wrapper.
pub trait GetRomfile {
    fn romfile(&self) -> &CommonRomfile;
}

impl GetRomfile for CommonRomfile {
    fn romfile(&self) -> &CommonRomfile {
        self
    }
}

impl GetRomfile for IsoRomfile {
    fn romfile(&self) -> &CommonRomfile {
        &self.romfile
    }
}

// === CONVERSION TRAITS ===
pub trait AsCommon {
    async fn as_common(&self, connection: &mut SqliteConnection) -> Result<CommonRomfile>;
}

pub trait ToCommon {
    async fn to_common<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<CommonRomfile>;
}

pub trait AsIso {
    fn as_iso(self) -> Result<IsoRomfile>;
}

pub trait AsCueBin {
    fn as_cue_bin(self, bin_romfiles: Vec<CommonRomfile>) -> Result<CueBinRomfile>;
}

pub trait ToIso {
    async fn to_iso<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<IsoRomfile>;
}

pub trait ToCueBin {
    async fn to_cue_bin<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        cue_romfile: Option<CommonRomfile>,
        bin_roms: &[&Rom],
        quiet: bool,
    ) -> Result<CueBinRomfile>;
}

// === SPECIALIZED TRAITS ===
// patch application is not wired up yet, kept for the planned feature
#[allow(dead_code)]
pub trait Patch {
    async fn patch<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        romfile: &CommonRomfile,
        destination_directory: &P,
    ) -> Result<CommonRomfile>;
}

pub trait Playlist {
    async fn get_playlist_path(
        &self,
        connection: &mut SqliteConnection,
        system: &System,
        subfolders: &Option<SubfolderScheme>,
    ) -> Result<PathBuf>;
}

pub trait Size {
    async fn get_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
    ) -> Result<u64>;
}

pub trait HashAndSize {
    async fn get_hash_and_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> Result<(String, u64)>;
}

pub trait HeaderedHashAndSize {
    async fn get_file_and_header_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Header,
    ) -> Result<(File, u64)>;

    async fn get_headered_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Header,
    ) -> Result<u64>;

    async fn get_headered_hash_and_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Header,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> Result<(String, u64)>;
}

pub trait Check {
    async fn check(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        roms: &[&Rom],
    ) -> Result<()>;
}

pub trait Persist {
    async fn create(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        romfile_type: RomfileType,
    ) -> Result<i64>;

    async fn update(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        id: i64,
    ) -> Result<()>;
}

// === IMPLEMENTATIONS FOR CommonRomfile ===
impl FromPath<CommonRomfile> for CommonRomfile {
    fn from_path<P: AsRef<Path>>(path: &P) -> Result<CommonRomfile> {
        Ok(CommonRomfile {
            path: path.as_ref().to_path_buf(),
            system_id: None,
        })
    }
}

impl fmt::Display for CommonRomfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.path.as_os_str().to_str().unwrap())
    }
}

impl CommonFile for CommonRomfile {
    async fn get_relative_path(&self, connection: &mut SqliteConnection) -> Result<&Path> {
        let rom_directory = match get_directory(connection, "ROM_DIRECTORY", self.system_id).await {
            Some(dir) => dir,
            None => get_rom_directory(connection).await,
        };
        let relative_path = self.path.strip_prefix(rom_directory).with_context(|| {
            format!(
                "Failed to convert \"{}\"to relative path",
                self.path.as_os_str().to_str().unwrap()
            )
        })?;
        Ok(relative_path)
    }

    async fn rename<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        new_path: &P,
        quiet: bool,
    ) -> Result<CommonRomfile> {
        if self.path != new_path.as_ref() {
            rename_file(progress_bar, &self.path, new_path, quiet).await?;
        }
        CommonRomfile::from_path(new_path)
    }

    async fn copy<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        quiet: bool,
    ) -> Result<CommonRomfile> {
        let new_path = destination_directory
            .as_ref()
            .join(self.path.file_name().unwrap());
        copy_file(progress_bar, &self.path, &new_path, quiet).await?;
        CommonRomfile::from_path(&new_path)
    }

    async fn delete(&self, progress_bar: &ProgressBar, quiet: bool) -> Result<()> {
        remove_file(progress_bar, &self.path, quiet).await?;
        Ok(())
    }

    async fn get_sorted_path(
        &self,
        connection: &mut SqliteConnection,
        system: &System,
        game: &Game,
        rom: &Rom,
        subfolder_scheme: &Option<SubfolderScheme>,
        extension: &Option<&str>,
    ) -> Result<PathBuf> {
        let extension = extension.or_else(|| self.path.extension()?.to_str());

        // sorting
        let mut sorted_path = match game.sorting {
            s if s == Sorting::OneRegion as i64 => {
                get_one_region_directory(connection, system).await?
            }
            s if s == Sorting::Ignored as i64 => {
                get_trash_directory(connection, Some(system)).await?
            }
            _ => get_system_directory(connection, system).await?,
        };

        // subfolders
        let subfolder_scheme = match subfolder_scheme {
            Some(scheme) => scheme,
            None => match Sorting::from_i64(game.sorting) {
                Some(Sorting::OneRegion) => &SubfolderScheme::from_str(
                    &get_string(connection, "REGIONS_ONE_SUBFOLDERS", Some(system.id))
                        .await
                        .unwrap(),
                )
                .unwrap(),
                Some(Sorting::AllRegions) => &SubfolderScheme::from_str(
                    &get_string(connection, "REGIONS_ALL_SUBFOLDERS", Some(system.id))
                        .await
                        .unwrap(),
                )
                .unwrap(),
                Some(Sorting::Ignored) | None => &SubfolderScheme::None,
            },
        };
        if subfolder_scheme == &SubfolderScheme::Alpha {
            sorted_path.push(compute_alpha_subfolder(&game.name));
        }

        // arcade and jbfolder in subdirectories unless they are archives
        if (system.arcade && !extension.is_some_and(|ext| ARCHIVE_EXTENSIONS.contains(&ext)))
            || game.jbfolder
        {
            sorted_path.push(&game.name);
        }

        // file name
        let filename = match extension {
            Some(ext) if NON_ORIGINAL_EXTENSIONS.contains(&ext) => {
                if system.arcade && !ARCHIVE_EXTENSIONS.contains(&ext) {
                    format!("{}.{}", rom.name, ext)
                } else {
                    format!("{}.{}", game.name, ext)
                }
            }
            _ => match &system.custom_extension {
                Some(custom_ext) => format!("{}.{}", game.name, custom_ext),
                None => rom.name.clone(),
            },
        };

        sorted_path.push(filename);
        Ok(sorted_path)
    }
}

impl Size for CommonRomfile {
    async fn get_size(
        &self,
        _connection: &mut SqliteConnection,
        _progress_bar: &ProgressBar,
    ) -> Result<u64> {
        Ok(self.path.metadata().unwrap().len())
    }
}

impl HashAndSize for CommonRomfile {
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

        let mut file = open_file_sync(&self.path)?;
        let (hash, _) = hash_reader(&mut file, progress_bar, hash_algorithm)?;
        let size = self.get_size(connection, progress_bar).await?;

        progress_bar.set_message("");

        Ok((hash, size))
    }
}

/// Hash everything `reader` yields, advancing `progress_bar` as it goes, and
/// return the hash alongside the number of bytes read.
///
/// Kept separate from any particular romfile so that a backend able to decode a
/// disc image on the fly can hash the stream without writing it out first.
pub fn hash_reader<R: Read + ?Sized>(
    reader: &mut R,
    progress_bar: &ProgressBar,
    hash_algorithm: &HashAlgorithm,
) -> Result<(String, u64)> {
    let size;
    let hash = match hash_algorithm {
        HashAlgorithm::Crc => {
            let mut digest = Crc32::new();
            size = io::copy(reader, &mut progress_bar.wrap_write(&mut digest))
                .context("Failed to copy data")?;
            to_hex(&digest.finalize())
        }
        HashAlgorithm::Md5 => {
            let mut digest = IoWrapper(Md5::new());
            size = io::copy(reader, &mut progress_bar.wrap_write(&mut digest))
                .context("Failed to copy data")?;
            to_hex(&digest.0.finalize())
        }
        HashAlgorithm::Sha1 => {
            let mut digest = IoWrapper(Sha1::new());
            size = io::copy(reader, &mut progress_bar.wrap_write(&mut digest))
                .context("Failed to copy data")?;
            to_hex(&digest.0.finalize())
        }
    };
    Ok((hash, size))
}

/// The algorithm to verify `rom` with, picked from whichever checksum the
/// database holds for it.
pub fn get_hash_algorithm(rom: &Rom) -> Result<HashAlgorithm> {
    if rom.crc.is_some() {
        Ok(HashAlgorithm::Crc)
    } else if rom.md5.is_some() {
        Ok(HashAlgorithm::Md5)
    } else if rom.sha1.is_some() {
        Ok(HashAlgorithm::Sha1)
    } else {
        bail!("Not possible")
    }
}

/// Compare a computed hash and size against what the database expects of `rom`.
pub fn compare_hash_and_size(
    rom: &Rom,
    hash: &str,
    size: u64,
    hash_algorithm: &HashAlgorithm,
) -> Result<()> {
    if rom.size > 0 && size != rom.size as u64 {
        bail!("Size mismatch");
    };
    let expected = match hash_algorithm {
        HashAlgorithm::Crc => rom.crc.as_ref(),
        HashAlgorithm::Md5 => rom.md5.as_ref(),
        HashAlgorithm::Sha1 => rom.sha1.as_ref(),
    };
    if Some(&hash.to_string()) != expected {
        bail!("Checksum mismatch");
    }
    Ok(())
}

impl HeaderedHashAndSize for CommonRomfile {
    async fn get_file_and_header_size(
        &self,
        connection: &mut SqliteConnection,
        _progress_bar: &ProgressBar,
        header: &Header,
    ) -> Result<(File, u64)> {
        let mut file = open_file_sync(&self.path)?;
        let mut header_size: u64 = 0;

        // extract a potential header, revert if none is found
        let rules = find_rules_by_header_id(connection, header.id).await;
        let mut buffer: Vec<u8> = Vec::with_capacity(header.size as usize);
        (&mut file)
            .take(header.size as u64)
            .read_to_end(&mut buffer)
            .context("Failed to read into buffer")?;

        let mut matches: Vec<bool> = vec![];
        for rule in rules {
            let start_byte = rule.start_byte as usize;
            let hex_values: Vec<String> = buffer[start_byte..]
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect();
            let hex_value = hex_values.join("").to_lowercase();
            matches.push(hex_value.starts_with(&rule.hex_value.to_lowercase()));
        }

        if matches.iter().all(|&m| m) {
            header_size = header.size as u64;
        } else {
            file.rewind().context("Failed to rewind file")?;
        }

        Ok((file, header_size))
    }

    async fn get_headered_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Header,
    ) -> Result<u64> {
        let (file, header_size) = self
            .get_file_and_header_size(connection, progress_bar, header)
            .await?;
        Ok(file.metadata().unwrap().len() - header_size)
    }

    async fn get_headered_hash_and_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Header,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> Result<(String, u64)> {
        let size = self
            .get_headered_size(connection, progress_bar, header)
            .await?;

        progress_bar.reset();
        progress_bar.set_message(format!(
            "Computing {} ({}/{})",
            hash_algorithm, position, total
        ));
        progress_bar.set_style(get_bytes_progress_style());
        progress_bar.set_length(size);

        let mut file = self
            .get_file_and_header_size(connection, progress_bar, header)
            .await?
            .0;
        let hash = match hash_algorithm {
            HashAlgorithm::Crc => {
                let mut digest = Crc32::new();
                io::copy(&mut file, &mut progress_bar.wrap_write(&mut digest))
                    .context("Failed to copy data")?;
                to_hex(&digest.finalize())
            }
            HashAlgorithm::Md5 => {
                let mut digest = IoWrapper(Md5::new());
                io::copy(&mut file, &mut progress_bar.wrap_write(&mut digest))
                    .context("Failed to copy data")?;
                to_hex(&digest.0.finalize())
            }
            HashAlgorithm::Sha1 => {
                let mut digest = IoWrapper(Sha1::new());
                io::copy(&mut file, &mut progress_bar.wrap_write(&mut digest))
                    .context("Failed to copy data")?;
                to_hex(&digest.0.finalize())
            }
        };

        progress_bar.set_message("");
        progress_bar.set_style(get_none_progress_style());

        Ok((hash, size))
    }
}

impl Check for CommonRomfile {
    async fn check(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        roms: &[&Rom],
    ) -> Result<()> {
        print_action(progress_bar, &format!("Checking \"{}\"", self));
        let rom = roms[0];
        let hash_algorithm = get_hash_algorithm(rom)?;
        let (hash, size) = match header {
            Some(header) => {
                self.get_headered_hash_and_size(
                    connection,
                    progress_bar,
                    header,
                    1,
                    1,
                    &hash_algorithm,
                )
                .await?
            }
            None => {
                let (hash, size) = self
                    .get_hash_and_size(connection, progress_bar, 1, 1, &hash_algorithm)
                    .await?;
                (hash, size)
            }
        };
        compare_hash_and_size(rom, &hash, size, &hash_algorithm)
    }
}

impl Persist for CommonRomfile {
    async fn create(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        romfile_type: RomfileType,
    ) -> Result<i64> {
        let path = &self.get_relative_path(connection).await?;
        let size = self.get_size(connection, progress_bar).await?;
        Ok(create_romfile(
            connection,
            path.as_os_str().to_str().unwrap(),
            size,
            romfile_type,
        )
        .await)
    }

    async fn update(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        id: i64,
    ) -> Result<()> {
        let path = &self.get_relative_path(connection).await?;
        let size = self.get_size(connection, progress_bar).await?;
        update_romfile(connection, id, path.as_os_str().to_str().unwrap(), size).await;
        Ok(())
    }
}

// === CONVERSION IMPLEMENTATIONS ===
impl AsCommon for Romfile {
    async fn as_common(&self, connection: &mut SqliteConnection) -> Result<CommonRomfile> {
        let rom_directory = get_rom_directory(connection).await;
        CommonRomfile::from_path(&rom_directory.join(&self.path))
    }
}

impl AsIso for CommonRomfile {
    fn as_iso(self) -> Result<IsoRomfile> {
        if self
            .path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase()
            != ISO_EXTENSION
        {
            bail!("Not a valid iso");
        }
        Ok(IsoRomfile { romfile: self })
    }
}

impl AsCueBin for CommonRomfile {
    fn as_cue_bin(self, bin_romfiles: Vec<CommonRomfile>) -> Result<CueBinRomfile> {
        if self
            .path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase()
            != CUE_EXTENSION
        {
            bail!("Not a valid cue");
        }
        for bin_romfile in &bin_romfiles {
            if bin_romfile
                .path
                .extension()
                .unwrap()
                .to_str()
                .unwrap()
                .to_lowercase()
                != BIN_EXTENSION
            {
                bail!("Not a valid bin");
            }
        }
        Ok(CueBinRomfile {
            cue_romfile: self,
            bin_romfiles,
        })
    }
}

// === PLAYLIST IMPLEMENTATIONS ===
impl Playlist for Game {
    async fn get_playlist_path(
        &self,
        connection: &mut SqliteConnection,
        system: &System,
        subfolder_scheme: &Option<SubfolderScheme>,
    ) -> Result<PathBuf> {
        let mut playlist_path = if self.sorting == Sorting::OneRegion as i64 {
            get_one_region_directory(connection, system).await?
        } else {
            get_system_directory(connection, system).await?
        };
        let subfolder_scheme = match subfolder_scheme {
            Some(scheme) => scheme,
            None => match Sorting::from_i64(self.sorting) {
                Some(Sorting::OneRegion) => &SubfolderScheme::from_str(
                    &get_string(connection, "REGIONS_ONE_SUBFOLDERS", Some(system.id))
                        .await
                        .unwrap(),
                )
                .unwrap(),
                Some(Sorting::AllRegions) => &SubfolderScheme::from_str(
                    &get_string(connection, "REGIONS_ALL_SUBFOLDERS", Some(system.id))
                        .await
                        .unwrap(),
                )
                .unwrap(),
                Some(Sorting::Ignored) | None => &SubfolderScheme::None,
            },
        };
        if subfolder_scheme == &SubfolderScheme::Alpha {
            playlist_path = playlist_path.join(compute_alpha_subfolder(&self.name));
        }
        playlist_path = playlist_path.join(format!(
            "{}.{}",
            DISC_REGEX.replace(&self.name, ""),
            M3U_EXTENSION
        ));
        Ok(playlist_path)
    }
}

// === TESTS ===
#[cfg(test)]
mod test_path_archive_custom_extension;
#[cfg(test)]
mod test_path_archive_multiple_files;
#[cfg(test)]
mod test_path_archive_single_file;
#[cfg(test)]
mod test_path_chd_multiple_tracks;
#[cfg(test)]
mod test_path_chd_single_track;
#[cfg(test)]
mod test_path_cso;
#[cfg(test)]
mod test_path_custom_extension;
#[cfg(test)]
mod test_path_original;
#[cfg(test)]
mod test_path_playlist;
#[cfg(test)]
mod test_path_playlist_subfolder_alpha;
#[cfg(test)]
mod test_path_rvz;
#[cfg(test)]
mod test_path_subfolder_alpha_letter;
#[cfg(test)]
mod test_path_subfolder_alpha_other;

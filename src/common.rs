// === IMPORTS ===
use super::config::*;
use super::crc32::*;
use super::database::*;
use super::generate_playlists::DISC_REGEX;
use super::mimetype::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use core::fmt;
use digest::Digest;
use indicatif::ProgressBar;
use md5::Md5;
use num_traits::FromPrimitive;
use sha1::Sha1;
use simple_error::SimpleResult;
use sqlx::SqliteConnection;
use std::io;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::{fs::File, str::FromStr};

// === CORE TYPES ===
#[derive(Clone)]
pub struct CommonRomfile {
    pub path: PathBuf,
}

pub struct M3uRomfile {
    pub romfile: CommonRomfile,
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
    fn from_path<P: AsRef<Path>>(path: &P) -> SimpleResult<T>;
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
    ) -> SimpleResult<PathBuf>;

    async fn get_relative_path(&self, connection: &mut SqliteConnection) -> SimpleResult<&Path>;

    async fn rename<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        new_path: &P,
        quiet: bool,
    ) -> SimpleResult<CommonRomfile>;

    async fn copy<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        quiet: bool,
    ) -> SimpleResult<CommonRomfile>;

    async fn delete(&self, progress_bar: &ProgressBar, quiet: bool) -> SimpleResult<()>;
}

// === CONVERSION TRAITS ===
pub trait AsCommon {
    async fn as_common(&self, connection: &mut SqliteConnection) -> SimpleResult<CommonRomfile>;
}

pub trait ToCommon {
    async fn to_common<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> SimpleResult<CommonRomfile>;
}

pub trait AsM3u {
    async fn as_m3u(self) -> SimpleResult<M3uRomfile>;
}

pub trait AsIso {
    fn as_iso(self) -> SimpleResult<IsoRomfile>;
}

pub trait AsCueBin {
    fn as_cue_bin(self, bin_romfiles: Vec<CommonRomfile>) -> SimpleResult<CueBinRomfile>;
}



pub trait ToIso {
    async fn to_iso<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> SimpleResult<IsoRomfile>;
}

pub trait ToCueBin {
    async fn to_cue_bin<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
        cue_romfile: Option<CommonRomfile>,
        bin_roms: &[&Rom],
        quiet: bool,
    ) -> SimpleResult<CueBinRomfile>;
}



// === SPECIALIZED TRAITS ===
pub trait Patch {
    async fn patch<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        romfile: &CommonRomfile,
        destination_directory: &P,
    ) -> SimpleResult<CommonRomfile>;
}

pub trait Playlist {
    async fn get_playlist_path(
        &self,
        connection: &mut SqliteConnection,
        system: &System,
        subfolders: &Option<SubfolderScheme>,
    ) -> SimpleResult<PathBuf>;
}

pub trait Size {
    async fn get_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
    ) -> SimpleResult<u64>;
}

pub trait HashAndSize {
    async fn get_hash_and_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> SimpleResult<(String, u64)>;
}

pub trait HeaderedHashAndSize {
    async fn get_file_and_header_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Header,
    ) -> SimpleResult<(File, u64)>;

    async fn get_headered_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Header,
    ) -> SimpleResult<u64>;

    async fn get_headered_hash_and_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Header,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> SimpleResult<(String, u64)>;
}

pub trait Check {
    async fn check(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        roms: &[&Rom],
    ) -> SimpleResult<()>;
}

pub trait Persist {
    async fn create(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        romfile_type: RomfileType,
    ) -> SimpleResult<i64>;

    async fn update(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        id: i64,
    ) -> SimpleResult<()>;
}

// === IMPLEMENTATIONS FOR CommonRomfile ===
impl FromPath<CommonRomfile> for CommonRomfile {
    fn from_path<P: AsRef<Path>>(path: &P) -> SimpleResult<CommonRomfile> {
        Ok(CommonRomfile {
            path: path.as_ref().to_path_buf(),
        })
    }
}

impl fmt::Display for CommonRomfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.path.as_os_str().to_str().unwrap())
    }
}

impl CommonFile for CommonRomfile {
    async fn get_relative_path(&self, connection: &mut SqliteConnection) -> SimpleResult<&Path> {
        let rom_directory = get_rom_directory(connection).await;
        let relative_path = try_with!(
            self.path.strip_prefix(rom_directory),
            "Failed to convert \"{}\"to relative path",
            &self.path.as_os_str().to_str().unwrap()
        );
        Ok(relative_path)
    }

    async fn rename<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        new_path: &P,
        quiet: bool,
    ) -> SimpleResult<CommonRomfile> {
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
    ) -> SimpleResult<CommonRomfile> {
        let new_path = destination_directory
            .as_ref()
            .join(self.path.file_name().unwrap());
        copy_file(progress_bar, &self.path, &new_path, quiet).await?;
        CommonRomfile::from_path(&new_path)
    }

    async fn delete(&self, progress_bar: &ProgressBar, quiet: bool) -> SimpleResult<()> {
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
    ) -> SimpleResult<PathBuf> {
        let mut sorted_path = get_system_directory(connection, system).await?;

        let extension = extension.or_else(|| self.path.extension()?.to_str());

        // sorting
        match game.sorting {
            s if s == Sorting::OneRegion as i64 => {
                sorted_path = sorted_path.join("1G1R");
            }
            s if s == Sorting::Ignored as i64 => {
                sorted_path = sorted_path.join("Trash");
            }
            _ => {}
        }

        // subfolders
        let subfolder_scheme = match subfolder_scheme {
            Some(scheme) => scheme,
            None => match Sorting::from_i64(game.sorting) {
                Some(Sorting::OneRegion) => &SubfolderScheme::from_str(
                    &get_string(connection, "REGIONS_ONE_SUBFOLDERS")
                        .await
                        .unwrap(),
                )
                .unwrap(),
                Some(Sorting::AllRegions) => &SubfolderScheme::from_str(
                    &get_string(connection, "REGIONS_ALL_SUBFOLDERS")
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
        if (system.arcade && !extension.map_or(false, |ext| ARCHIVE_EXTENSIONS.contains(&ext)))
            || game.jbfolder
        {
            sorted_path.push(&game.name);
        }

        // file name
        let filename = match extension {
            Some(ext) if NON_ORIGINAL_EXTENSIONS.contains(&ext) => {
                if system.arcade && !ARCHIVE_EXTENSIONS.contains(&ext) {
                    format!("{}.{}", &rom.name, ext)
                } else {
                    format!("{}.{}", &game.name, ext)
                }
            }
            _ => match &system.custom_extension {
                Some(custom_ext) => format!("{}.{}", &game.name, custom_ext),
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
    ) -> SimpleResult<u64> {
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
    ) -> SimpleResult<(String, u64)> {
        progress_bar.reset();
        progress_bar.set_message(format!(
            "Computing {} ({}/{})",
            hash_algorithm, position, total
        ));

        let mut file = open_file_sync(&self.path)?;
        let hash = match hash_algorithm {
            HashAlgorithm::Crc => {
                let mut digest = Crc32::new();
                try_with!(
                    io::copy(&mut file, &mut progress_bar.wrap_write(&mut digest)),
                    "Failed to copy data"
                );
                format!("{:08x}", digest.finalize()).to_lowercase()
            }
            HashAlgorithm::Md5 => {
                let mut digest = Md5::new();
                try_with!(
                    io::copy(&mut file, &mut progress_bar.wrap_write(&mut digest)),
                    "Failed to copy data"
                );
                format!("{:032x}", digest.finalize()).to_lowercase()
            }
            HashAlgorithm::Sha1 => {
                let mut digest = Sha1::new();
                try_with!(
                    io::copy(&mut file, &mut progress_bar.wrap_write(&mut digest)),
                    "Failed to copy data"
                );
                format!("{:040x}", digest.finalize()).to_lowercase()
            }
        };
        let size = self.get_size(connection, progress_bar).await?;

        progress_bar.set_message("");

        Ok((hash, size))
    }
}

impl HeaderedHashAndSize for CommonRomfile {
    async fn get_file_and_header_size(
        &self,
        connection: &mut SqliteConnection,
        _progress_bar: &ProgressBar,
        header: &Header,
    ) -> SimpleResult<(File, u64)> {
        let mut file = open_file_sync(&self.path)?;
        let mut header_size: u64 = 0;

        // extract a potential header, revert if none is found
        let rules = find_rules_by_header_id(connection, header.id).await;
        let mut buffer: Vec<u8> = Vec::with_capacity(header.size as usize);
        try_with!(
            (&mut file)
                .take(header.size as u64)
                .read_to_end(&mut buffer),
            "Failed to read into buffer"
        );

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
            try_with!(file.rewind(), "Failed to rewind file");
        }

        Ok((file, header_size))
    }

    async fn get_headered_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Header,
    ) -> SimpleResult<u64> {
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
    ) -> SimpleResult<(String, u64)> {
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
                try_with!(
                    io::copy(&mut file, &mut progress_bar.wrap_write(&mut digest)),
                    "Failed to copy data"
                );
                format!("{:08x}", digest.finalize()).to_lowercase()
            }
            HashAlgorithm::Md5 => {
                let mut digest = Md5::new();
                try_with!(
                    io::copy(&mut file, &mut progress_bar.wrap_write(&mut digest)),
                    "Failed to copy data"
                );
                format!("{:032x}", digest.finalize()).to_lowercase()
            }
            HashAlgorithm::Sha1 => {
                let mut digest = Sha1::new();
                try_with!(
                    io::copy(&mut file, &mut progress_bar.wrap_write(&mut digest)),
                    "Failed to copy data"
                );
                format!("{:040x}", digest.finalize()).to_lowercase()
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
    ) -> SimpleResult<()> {
        progress_bar.println(format!("Checking \"{}\"", self));
        let rom = roms[0];
        let hash_algorithm: HashAlgorithm;
        if rom.crc.is_some() {
            hash_algorithm = HashAlgorithm::Crc;
        } else if rom.md5.is_some() {
            hash_algorithm = HashAlgorithm::Md5;
        } else if rom.sha1.is_some() {
            hash_algorithm = HashAlgorithm::Sha1;
        } else {
            bail!("Not possible")
        }
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
        if rom.size > 0 && size != rom.size as u64 {
            bail!("Size mismatch");
        };
        match hash_algorithm {
            HashAlgorithm::Crc => {
                if &hash != rom.crc.as_ref().unwrap() {
                    bail!("Checksum mismatch");
                }
            }
            HashAlgorithm::Md5 => {
                if &hash != rom.md5.as_ref().unwrap() {
                    bail!("Checksum mismatch");
                }
            }
            HashAlgorithm::Sha1 => {
                if &hash != rom.sha1.as_ref().unwrap() {
                    bail!("Checksum mismatch");
                }
            }
        }
        Ok(())
    }
}

impl Persist for CommonRomfile {
    async fn create(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        romfile_type: RomfileType,
    ) -> SimpleResult<i64> {
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
    ) -> SimpleResult<()> {
        let path = &self.get_relative_path(connection).await?;
        let size = self.get_size(connection, progress_bar).await?;
        update_romfile(connection, id, path.as_os_str().to_str().unwrap(), size).await;
        Ok(())
    }
}

// === CONVERSION IMPLEMENTATIONS ===
impl AsCommon for Romfile {
    async fn as_common(&self, connection: &mut SqliteConnection) -> SimpleResult<CommonRomfile> {
        let rom_directory = get_rom_directory(connection).await;
        CommonRomfile::from_path(&rom_directory.join(&self.path))
    }
}

impl AsM3u for CommonRomfile {
    async fn as_m3u(self) -> SimpleResult<M3uRomfile> {
        if self
            .path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase()
            != M3U_EXTENSION
        {
            bail!("Not a valid m3u");
        }
        Ok(M3uRomfile { romfile: self })
    }
}

impl AsIso for CommonRomfile {
    fn as_iso(self) -> SimpleResult<IsoRomfile> {
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
    fn as_cue_bin(self, bin_romfiles: Vec<CommonRomfile>) -> SimpleResult<CueBinRomfile> {
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
    ) -> SimpleResult<PathBuf> {
        let mut playlist_path = get_system_directory(connection, system).await?;
        let subfolder_scheme = match subfolder_scheme {
            Some(scheme) => scheme,
            None => match Sorting::from_i64(self.sorting) {
                Some(Sorting::OneRegion) => &SubfolderScheme::from_str(
                    &get_string(connection, "REGIONS_ONE_SUBFOLDERS")
                        .await
                        .unwrap(),
                )
                .unwrap(),
                Some(Sorting::AllRegions) => &SubfolderScheme::from_str(
                    &get_string(connection, "REGIONS_ALL_SUBFOLDERS")
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
        if self.sorting == Sorting::OneRegion as i64 {
            playlist_path = playlist_path.join("1G1R");
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
mod test_path_archive_custom_extension;
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







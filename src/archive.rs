use super::common::*;
use super::config::*;
use super::mimetype::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use anyhow::{Context, Result};
use indicatif::ProgressBar;
use sevenz_rust2::encoder_options::{Lzma2Options, ZstandardOptions};
use sevenz_rust2::{
    Archive, ArchiveEntry, ArchiveWriter, BlockDecoder, EncoderConfiguration, EncoderMethod,
    Password, SourceReader,
};
use sqlx::SqliteConnection;
use std::fs::{File, OpenOptions};
use std::io;
use std::iter::zip;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use strum::{Display, EnumString, VariantNames};
use tokio::task::spawn_blocking;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub const SEVENZIP_COMPRESSION_LEVEL_RANGE: [usize; 2] = [1, 9];
pub const ZIP_COMPRESSION_LEVEL_RANGE: [usize; 2] = [1, 9];
pub const ZSTD_COMPRESSION_LEVEL_RANGE: [usize; 2] = [1, 22];

/// Levels used when unset; 19 for Zstandard is what RomVault writes.
const DEFAULT_COMPRESSION_LEVEL: usize = 9;
const DEFAULT_ZSTD_COMPRESSION_LEVEL: usize = 19;

#[derive(Clone, Copy, Display, EnumString, PartialEq, Eq, VariantNames)]
#[strum(serialize_all = "lowercase")]
pub enum SevenzipCompressionAlgorithm {
    Lzma2,
    Zstd,
}

#[derive(Clone, Copy, Display, EnumString, PartialEq, Eq, VariantNames)]
#[strum(serialize_all = "lowercase")]
pub enum ZipCompressionAlgorithm {
    Deflate,
    Zstd,
}

/// An algorithm and its level, on that algorithm's own scale.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ArchiveCompression {
    Deflate(usize),
    Lzma2(usize),
    Zstd(usize),
    /// Whatever the crate picks for itself.
    Default,
}

/// Read the configured algorithm and level; Zstandard's 1-22 level has its own key.
pub async fn get_archive_compression(
    connection: &mut SqliteConnection,
    archive_type: &ArchiveType,
    system_id: Option<i64>,
) -> ArchiveCompression {
    let (algorithm_key, zstd_level_key, level_key) = match archive_type {
        ArchiveType::Sevenzip => (
            "SEVENZIP_COMPRESSION_ALGORITHM",
            "SEVENZIP_ZSTD_COMPRESSION_LEVEL",
            "SEVENZIP_COMPRESSION_LEVEL",
        ),
        ArchiveType::Zip => (
            "ZIP_COMPRESSION_ALGORITHM",
            "ZIP_ZSTD_COMPRESSION_LEVEL",
            "ZIP_COMPRESSION_LEVEL",
        ),
    };
    let algorithm = get_string(connection, algorithm_key, system_id).await;
    match algorithm.as_deref() {
        Some("zstd") => ArchiveCompression::Zstd(
            get_integer(connection, zstd_level_key, system_id)
                .await
                .unwrap_or(DEFAULT_ZSTD_COMPRESSION_LEVEL),
        ),
        Some("lzma2") => ArchiveCompression::Lzma2(
            get_integer(connection, level_key, system_id)
                .await
                .unwrap_or(DEFAULT_COMPRESSION_LEVEL),
        ),
        Some("deflate") => ArchiveCompression::Deflate(
            get_integer(connection, level_key, system_id)
                .await
                .unwrap_or(DEFAULT_COMPRESSION_LEVEL),
        ),
        _ => ArchiveCompression::Default,
    }
}

#[derive(Clone, Copy, Display, EnumString, PartialEq, Eq)]
#[strum(serialize_all = "lowercase")]
pub enum ArchiveType {
    #[strum(serialize = "7z")]
    Sevenzip,
    Zip,
}

pub struct ArchiveRomfile {
    pub romfile: CommonRomfile,
    pub path: String,
    pub archive_type: ArchiveType,
    pub size: u64,
    pub crc: String,
}

impl GetRomfile for ArchiveRomfile {
    fn romfile(&self) -> &CommonRomfile {
        &self.romfile
    }
}

pub trait ArchiveFile {
    async fn rename_file(
        &self,
        progress_bar: &ProgressBar,
        new_path: &str,
    ) -> Result<ArchiveRomfile>;
    async fn delete_file(&self, progress_bar: &ProgressBar) -> Result<()>;
}

impl ArchiveFile for ArchiveRomfile {
    async fn rename_file(
        &self,
        progress_bar: &ProgressBar,
        new_path: &str,
    ) -> Result<ArchiveRomfile> {
        start_action(progress_bar, Some("Renaming file in archive"));
        print_action(
            progress_bar,
            &format!("Renaming \"{}\" to \"{}\"", self.path, new_path),
        );

        rename(&self.romfile.path, &self.path, new_path).await?;

        stop_action(progress_bar);

        Ok(ArchiveRomfile {
            romfile: self.romfile.clone(),
            path: new_path.to_string(),
            archive_type: self.archive_type,
            size: self.size,
            crc: self.crc.clone(),
        })
    }

    async fn delete_file(&self, progress_bar: &ProgressBar) -> Result<()> {
        start_action(progress_bar, Some("Deleting files"));

        print_action(progress_bar, &format!("Deleting \"{}\"", self.path));

        delete(&self.romfile.path, &self.path).await?;

        stop_action(progress_bar);

        if self
            .romfile
            .as_archive(progress_bar, None)
            .await?
            .is_empty()
        {
            self.romfile.delete(progress_bar, false).await?;
        }

        Ok(())
    }
}

impl Size for ArchiveRomfile {
    async fn get_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
    ) -> Result<u64> {
        if self.size > 0 {
            Ok(self.size)
        } else {
            let tmp_directory = create_tmp_directory(connection).await?;
            let size = self
                .to_common(progress_bar, &tmp_directory)
                .await?
                .get_size(connection, progress_bar)
                .await?;
            Ok(size)
        }
    }
}

impl HashAndSize for ArchiveRomfile {
    async fn get_hash_and_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> Result<(String, u64)> {
        if hash_algorithm == &HashAlgorithm::Crc && !self.crc.is_empty() && self.size > 0 {
            Ok((self.crc.clone(), self.size))
        } else {
            let tmp_directory = create_tmp_directory(connection).await?;
            let (hash, size) = self
                .to_common(progress_bar, &tmp_directory)
                .await?
                .get_hash_and_size(connection, progress_bar, position, total, hash_algorithm)
                .await?;
            Ok((hash, size))
        }
    }
}

impl Check for ArchiveRomfile {
    async fn check(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        roms: &[&Rom],
    ) -> Result<()> {
        print_action(
            progress_bar,
            &format!("Checking \"{}\" ({})", self.romfile, self.path),
        );
        let tmp_directory = create_tmp_directory(connection).await?;
        let common_romfile = self.to_common(progress_bar, &tmp_directory).await?;
        common_romfile
            .check(connection, progress_bar, header, roms)
            .await?;
        Ok(())
    }
}

impl ToCommon for ArchiveRomfile {
    async fn to_common<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        directory: &P,
    ) -> Result<CommonRomfile> {
        start_action(progress_bar, Some("Extracting file"));

        print_action(progress_bar, &format!("Extracting \"{}\"", self.path));

        extract(&self.romfile.path, &self.path, directory.as_ref()).await?;

        stop_action(progress_bar);

        CommonRomfile::from_path(&directory.as_ref().join(&self.path))
    }
}

#[allow(clippy::too_many_arguments)]
pub trait ToArchive {
    async fn to_archive<P: AsRef<Path>, Q: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        working_directory: &P,
        destination_directory: &Q,
        archive_name: &str,
        archive_type: &ArchiveType,
        compression: &ArchiveCompression,
        solid: bool,
    ) -> Result<ArchiveRomfile>;
}

impl ToArchive for CommonRomfile {
    async fn to_archive<P: AsRef<Path>, Q: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        working_directory: &P,
        destination_directory: &Q,
        archive_name: &str,
        archive_type: &ArchiveType,
        compression: &ArchiveCompression,
        solid: bool,
    ) -> Result<ArchiveRomfile> {
        start_action(progress_bar, Some(&format!("Creating {}", archive_type)));

        print_action(progress_bar, &format!("Compressing \"{}\"", self));

        let archive_path = destination_directory.as_ref().join(format!(
            "{}.{}",
            archive_name,
            match archive_type {
                ArchiveType::Sevenzip => SEVENZIP_EXTENSION,
                ArchiveType::Zip => ZIP_EXTENSION,
            }
        ));
        let path = self.path.strip_prefix(working_directory).unwrap();

        create(
            &archive_path,
            working_directory.as_ref(),
            path,
            archive_type,
            compression,
            solid,
        )
        .await?;

        stop_action(progress_bar);

        Ok(ArchiveRomfile {
            romfile: CommonRomfile::from_path(&archive_path)?,
            path: path.as_os_str().to_str().unwrap().to_string(),
            archive_type: *archive_type,
            size: 0,
            crc: String::new(),
        })
    }
}

impl ToArchive for ArchiveRomfile {
    async fn to_archive<P: AsRef<Path>, Q: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        source_directory: &P,
        destination_directory: &Q,
        archive_name: &str,
        archive_type: &ArchiveType,
        compression: &ArchiveCompression,
        solid: bool,
    ) -> Result<ArchiveRomfile> {
        let original_romfile = self.to_common(progress_bar, source_directory).await?;
        let archive_romfile = original_romfile
            .to_archive(
                progress_bar,
                source_directory,
                destination_directory,
                archive_name,
                archive_type,
                compression,
                solid,
            )
            .await?;
        original_romfile.delete(progress_bar, true).await?;
        Ok(archive_romfile)
    }
}

pub trait AsArchive {
    async fn parse_archive(
        &self,
        progress_bar: &ProgressBar,
        rom: Option<&Rom>,
    ) -> Result<Vec<(String, u64, String)>>;
    async fn as_archive(
        &self,
        progress_bar: &ProgressBar,
        rom: Option<&Rom>,
    ) -> Result<Vec<ArchiveRomfile>>;
}

impl AsArchive for CommonRomfile {
    async fn parse_archive(
        &self,
        progress_bar: &ProgressBar,
        rom: Option<&Rom>,
    ) -> Result<Vec<(String, u64, String)>> {
        start_action(progress_bar, Some("Parsing archive"));

        let entries = parse(&self.path, rom.map(|rom| rom.name.as_str())).await?;

        stop_action(progress_bar);

        Ok(entries)
    }
    async fn as_archive(
        &self,
        progress_bar: &ProgressBar,
        rom: Option<&Rom>,
    ) -> Result<Vec<ArchiveRomfile>> {
        start_action(progress_bar, Some("Parsing archive"));

        let paths_sizes_crcs = self.parse_archive(progress_bar, rom).await?;

        let extension = self
            .path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase();
        let archive_type = ArchiveType::from_str(&extension).context("Not a valid archive")?;
        let archived_romfiles: Vec<ArchiveRomfile> = paths_sizes_crcs
            .into_iter()
            .map(|(path, size, crc)| ArchiveRomfile {
                romfile: self.clone(),
                path: path.to_string(),
                archive_type,
                size,
                crc,
            })
            .collect();

        stop_action(progress_bar);

        Ok(archived_romfiles)
    }
}

pub async fn copy_files_between_archives<P: AsRef<Path>, Q: AsRef<Path>>(
    progress_bar: &ProgressBar,
    source_archive_path: &P,
    destination_archive_path: &Q,
    source_names: &[&str],
    destination_names: &[&str],
) -> Result<()> {
    start_action(progress_bar, Some("Copying files between archives"));

    let source_archive_file =
        File::open(source_archive_path.as_ref()).expect("Failed to read archive");
    let mut source_archive = ZipArchive::new(source_archive_file).expect("Failed to open archive");

    let destination_archive_file: File;
    let mut destination_archive: ZipWriter<File>;
    if destination_archive_path.as_ref().is_file() {
        destination_archive_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(destination_archive_path.as_ref())
            .expect("Failed to open archive");
        destination_archive =
            ZipWriter::new_append(destination_archive_file).expect("Failed to open archive");
    } else {
        destination_archive_file =
            File::create(destination_archive_path.as_ref()).expect("Failed to create archive");
        destination_archive = ZipWriter::new(destination_archive_file);
    };

    for (&source_name, &destination_name) in zip(source_names, destination_names) {
        if source_name == destination_name {
            print_action(progress_bar, &format!("Copying \"{}\"", source_name));
            destination_archive
                .raw_copy_file(source_archive.by_name(source_name).unwrap())
                .expect("Failed to copy file")
        } else {
            print_action(
                progress_bar,
                &format!("Copying \"{}\" to \"{}\"", source_name, destination_name),
            );
            destination_archive
                .raw_copy_file_rename(
                    source_archive.by_name(source_name).unwrap(),
                    destination_name,
                )
                .expect("Failed to copy file")
        }
    }

    Ok(())
}

/// Reported by `info`.
pub async fn get_version() -> Result<String> {
    Ok(String::from("built-in"))
}

fn type_of(path: &Path) -> Result<ArchiveType> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_lowercase();
    ArchiveType::from_str(&extension).context("Not a valid archive")
}

/// List entries as `(path, size, crc)`, the CRC as eight lowercase hex digits.
async fn parse<P: AsRef<Path>>(path: P, name: Option<&str>) -> Result<Vec<(String, u64, String)>> {
    let path = path.as_ref().to_path_buf();
    let name = name.map(|name| name.to_string());
    spawn_blocking(move || {
        let wanted = |entry: &str| name.as_ref().is_none_or(|name| name == entry);
        match type_of(&path)? {
            ArchiveType::Sevenzip => Ok(open_sevenzip(&path)?
                .files
                .iter()
                .filter(|entry| !entry.is_directory && wanted(&entry.name))
                .map(|entry| {
                    (
                        entry.name.clone(),
                        entry.size,
                        format!("{:08x}", entry.crc as u32),
                    )
                })
                .collect()),
            ArchiveType::Zip => {
                let mut archive = open_zip(&path)?;
                let mut entries = Vec::with_capacity(archive.len());
                for index in 0..archive.len() {
                    let entry = archive.by_index(index)?;
                    if entry.is_dir() || !wanted(entry.name()) {
                        continue;
                    }
                    entries.push((
                        entry.name().to_string(),
                        entry.size(),
                        format!("{:08x}", entry.crc32()),
                    ));
                }
                Ok(entries)
            }
        }
    })
    .await
    .context("Archive task failed")?
}

async fn extract<P: AsRef<Path>, Q: AsRef<Path>>(path: P, name: &str, directory: Q) -> Result<()> {
    let path = path.as_ref().to_path_buf();
    let name = name.to_string();
    let directory = directory.as_ref().to_path_buf();
    spawn_blocking(move || {
        let destination = directory.join(&name);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match type_of(&path)? {
            ArchiveType::Sevenzip => {
                let archive = open_sevenzip(&path)?;
                let index = archive
                    .files
                    .iter()
                    .position(|entry| entry.name == name)
                    .with_context(|| format!("No \"{}\" in \"{}\"", name, path.display()))?;
                // Entries share their block's stream, so skipped ones are drained.
                match archive.stream_map.file_block_index[index] {
                    Some(block) => {
                        let mut source = File::open(&path)?;
                        let password = Password::empty();
                        let mut decoder =
                            BlockDecoder::new(threads(), block, &archive, &password, &mut source);
                        decoder.set_thread_count(threads());
                        decoder.for_each_entries(&mut |entry, data| {
                            if entry.name != name {
                                io::copy(data, &mut io::sink())?;
                                return Ok(true);
                            }
                            io::copy(data, &mut File::create(&destination)?)?;
                            Ok(false)
                        })?;
                    }
                    None => {
                        File::create(&destination)?;
                    }
                }
            }
            ArchiveType::Zip => {
                let mut archive = open_zip(&path)?;
                let mut entry = archive
                    .by_name(&name)
                    .with_context(|| format!("Failed to extract \"{}\"", name))?;
                io::copy(&mut entry, &mut File::create(&destination)?)?;
            }
        }
        Ok(())
    })
    .await
    .context("Archive task failed")?
}

/// Add an entry to an archive, creating it if needed. `solid` is unused: entries
/// come one call at a time.
// TODO: batch `to_archive` so a game's files go into one solid block.
async fn create<P: AsRef<Path>, Q: AsRef<Path>>(
    archive_path: P,
    working_directory: Q,
    entry_path: &Path,
    archive_type: &ArchiveType,
    compression: &ArchiveCompression,
    _solid: bool,
) -> Result<()> {
    let archive_path = archive_path.as_ref().to_path_buf();
    let source = working_directory.as_ref().join(entry_path);
    let name = entry_path.to_string_lossy().replace('\\', "/");
    let archive_type = *archive_type;
    let compression = *compression;
    spawn_blocking(move || match archive_type {
        ArchiveType::Zip => {
            let (method, level) = match compression {
                ArchiveCompression::Zstd(level) => (CompressionMethod::Zstd, Some(level as i64)),
                ArchiveCompression::Deflate(level) => {
                    (CompressionMethod::Deflated, Some(level as i64))
                }
                _ => (CompressionMethod::Deflated, None),
            };
            let options = SimpleFileOptions::default()
                .compression_method(method)
                .compression_level(level)
                .large_file(true);
            let mut writer = if archive_path.is_file() {
                ZipWriter::new_append(File::options().read(true).write(true).open(&archive_path)?)?
            } else {
                ZipWriter::new(File::create(&archive_path)?)
            };
            writer.start_file(&name, options)?;
            io::copy(&mut File::open(&source)?, &mut writer)?;
            writer.finish()?;
            Ok(())
        }
        ArchiveType::Sevenzip => {
            let methods = content_methods(compression);
            let add = |writer: &mut ArchiveWriter<File>| -> Result<usize> {
                writer.set_content_methods(methods.clone());
                writer.push_archive_entry(
                    ArchiveEntry::from_path(&source, name.clone()),
                    Some(File::open(&source)?),
                )?;
                Ok(1)
            };
            if archive_path.is_file() {
                rewrite_sevenzip(&archive_path, |entry| Some(entry.to_string()), add)
            } else {
                let mut writer = ArchiveWriter::create(&archive_path)
                    .with_context(|| format!("Failed to create \"{}\"", archive_path.display()))?;
                add(&mut writer)?;
                writer.finish()?;
                Ok(())
            }
        }
    })
    .await
    .context("Archive task failed")?
}

async fn rename<P: AsRef<Path>>(path: P, from: &str, to: &str) -> Result<()> {
    let (from, to) = (from.to_string(), to.to_string());
    rewrite(path, move |name| {
        Some(if name == from {
            to.clone()
        } else {
            name.to_string()
        })
    })
    .await
}

async fn delete<P: AsRef<Path>>(path: P, name: &str) -> Result<()> {
    let name = name.to_string();
    rewrite(path, move |entry| {
        (entry != name).then(|| entry.to_string())
    })
    .await
}

/// Write an archive anew, passing entry names through `map` (`None` drops one).
/// Everything is copied raw; an archive left empty is removed.
async fn rewrite<P, F>(path: P, map: F) -> Result<()>
where
    P: AsRef<Path>,
    F: Fn(&str) -> Option<String> + Send + 'static,
{
    let path = path.as_ref().to_path_buf();
    spawn_blocking(move || match type_of(&path)? {
        ArchiveType::Zip => {
            let mut archive = open_zip(&path)?;
            let temporary = temporary_path(&path);
            let mut writer = ZipWriter::new(File::create(&temporary)?);
            let mut kept = 0;
            for index in 0..archive.len() {
                let entry = archive.by_index_raw(index)?;
                match map(entry.name()) {
                    Some(name) if name == entry.name() => writer.raw_copy_file(entry)?,
                    Some(name) => writer.raw_copy_file_rename(entry, name)?,
                    None => continue,
                }
                kept += 1;
            }
            writer.finish()?;
            replace(&path, &temporary, kept)
        }
        ArchiveType::Sevenzip => rewrite_sevenzip(&path, map, |_| Ok(0)),
    })
    .await
    .context("Archive task failed")?
}

/// Blocks `map` keeps whole are copied raw; a solid block it drops entries from
/// is re-encoded. `add` then writes new entries and returns how many.
fn rewrite_sevenzip<F>(
    path: &Path,
    map: F,
    add: impl FnOnce(&mut ArchiveWriter<File>) -> Result<usize>,
) -> Result<()>
where
    F: Fn(&str) -> Option<String>,
{
    let archive = open_sevenzip(path)?;
    let mut source = File::open(path)?;
    let temporary = temporary_path(path);
    let mut writer = ArchiveWriter::create(&temporary)
        .with_context(|| format!("Failed to create \"{}\"", temporary.display()))?;
    writer.set_content_methods(content_methods(if is_zstd(&archive) {
        ArchiveCompression::Zstd(DEFAULT_ZSTD_COMPRESSION_LEVEL)
    } else {
        ArchiveCompression::Default
    }));

    let mut kept = 0;
    for block in 0..archive.blocks.len() {
        let names: Vec<(&ArchiveEntry, Option<String>)> = archive
            .files
            .iter()
            .zip(&archive.stream_map.file_block_index)
            .filter(|(_, index)| **index == Some(block))
            .map(|(entry, _)| (entry, map(&entry.name)))
            .collect();
        let kept_here = names.iter().filter(|(_, name)| name.is_some()).count();
        kept += kept_here;
        if kept_here == names.len() {
            writer.push_raw_block(&archive, block, &mut source, |entry| {
                map(&entry.name).unwrap_or_default()
            })?;
        } else if kept_here > 0 {
            reencode(&archive, block, &mut source, &mut writer, &map, path)?;
        }
    }
    // Directories and empty files belong to no block.
    for (entry, _) in archive
        .files
        .iter()
        .zip(&archive.stream_map.file_block_index)
        .filter(|(_, index)| index.is_none())
    {
        if let Some(name) = map(&entry.name) {
            writer.push_archive_entry::<&[u8]>(
                ArchiveEntry {
                    name,
                    ..entry.clone()
                },
                None,
            )?;
            kept += 1;
        }
    }

    kept += add(&mut writer)?;
    writer.finish()?;
    replace(path, &temporary, kept)
}

/// Re-encode what `map` keeps of a block, via a scratch directory.
fn reencode<F>(
    archive: &Archive,
    block: usize,
    source: &mut File,
    writer: &mut ArchiveWriter<File>,
    map: &F,
    path: &Path,
) -> Result<()>
where
    F: Fn(&str) -> Option<String>,
{
    let scratch = tempfile::TempDir::new_in(path.parent().unwrap_or(Path::new(".")))?;
    let mut entries = Vec::new();
    let mut files = Vec::new();
    let password = Password::empty();
    let mut decoder = BlockDecoder::new(threads(), block, archive, &password, source);
    decoder.set_thread_count(threads());
    decoder.for_each_entries(&mut |entry, data| {
        match map(&entry.name) {
            Some(name) => {
                let file = scratch.path().join(entries.len().to_string());
                io::copy(data, &mut File::create(&file)?)?;
                entries.push(ArchiveEntry {
                    name,
                    ..entry.clone()
                });
                files.push(file);
            }
            None => {
                io::copy(data, &mut io::sink())?;
            }
        }
        Ok(true)
    })?;
    let readers = files
        .iter()
        .map(|file| File::open(file).map(SourceReader::from))
        .collect::<io::Result<Vec<_>>>()?;
    writer.push_archive_entries(entries, readers)?;
    Ok(())
}

fn replace(path: &Path, temporary: &Path, kept: usize) -> Result<()> {
    if kept == 0 {
        std::fs::remove_file(temporary)?;
        std::fs::remove_file(path)?;
    } else {
        std::fs::rename(temporary, path)?;
    }
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    path.with_extension("oxyromon-tmp")
}

fn open_sevenzip(path: &Path) -> Result<Archive> {
    Archive::open(path).with_context(|| format!("Failed to open \"{}\"", path.display()))
}

fn open_zip(path: &Path) -> Result<ZipArchive<File>> {
    ZipArchive::new(
        File::open(path).with_context(|| format!("Failed to open \"{}\"", path.display()))?,
    )
    .with_context(|| format!("Failed to read \"{}\"", path.display()))
}

fn threads() -> u32 {
    std::thread::available_parallelism()
        .map(|available| available.get() as u32)
        .unwrap_or(1)
}

fn is_zstd(archive: &Archive) -> bool {
    archive.blocks.iter().any(|block| {
        block
            .coders
            .iter()
            .any(|coder| coder.encoder_method_id() == EncoderMethod::ID_ZSTD)
    })
}

/// A chunk size of 1 asks LZMA2 for the finest split its dictionary allows.
fn content_methods(compression: ArchiveCompression) -> Vec<EncoderConfiguration> {
    vec![match compression {
        ArchiveCompression::Zstd(level) => ZstandardOptions::from_level(level as u32).into(),
        ArchiveCompression::Lzma2(level) => {
            Lzma2Options::from_level_mt(level as u32, threads(), 1).into()
        }
        _ => Lzma2Options::from_level_mt(DEFAULT_COMPRESSION_LEVEL as u32, threads(), 1).into(),
    }]
}

#[cfg(test)]
mod test_rewrite;

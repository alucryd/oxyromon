//! Archive handling backed by pure Rust crates: [`sevenz_rust2`] for 7z and
//! [`zip`] for ZIP, in place of the 7-Zip executable.
//!
//! Compiled in by the `sevenz` feature. Both crates are synchronous, so every
//! entry point hands its work to the blocking pool rather than occupying a
//! runtime worker.
//!
//! **Listing** is what oxyromon does most — `check-roms` and `import-roms` do it
//! for every archive they touch — and it is where this pays off most plainly:
//! both crates read names, sizes and CRCs out of the archive metadata, turning a
//! process launch per archive into a fraction of a millisecond of parsing.
//!
//! ZIP is native throughout. So is reading 7z, and writing one from scratch.
//! What goes back to 7-Zip is the three 7z operations that would otherwise
//! re-encode data to achieve a metadata change: [`rename`], [`delete`], and
//! appending to an archive that already exists (see [`create`]). 7-Zip can
//! relabel or drop an entry, or add a block, while leaving the existing
//! compressed streams alone; sevenz-rust2 exposes no way to do that, so the
//! equivalent here is a full rebuild. Renaming one entry of the 28MB fixture
//! below measured 8.86s that way, against 0.05s for `7z rn`.
//!
//! Codec speed, by contrast, turned out not to be the problem. Measured against
//! `tests/Test Game (USA, Europe) (Full).7z` — 3 entries, 28,224,253 bytes
//! uncompressed, solid, LZMA2 — versus 7-Zip 26.02, both release builds, on a
//! 16 core machine, at level 9 (oxyromon's default):
//!
//! | operation | sevenz-rust2 | 7-Zip, 1 thread | 7-Zip, all threads |
//! |-----------|--------------|-----------------|--------------------|
//! | read header | 0.22ms | needs a process launch | — |
//! | decompress | 0.31s (86 MB/s) | 0.24s | 0.24s |
//! | compress | 7.15s -> 6,337,607 B | 6.03s -> 6,333,772 B | 2.46s -> 6,333,873 B |
//!
//! So roughly 1.2x behind single threaded 7-Zip and 2.9x behind it with every
//! core, at a compressed size within 0.06%. That is an ordinary price for
//! dropping an external dependency.
//!
//! Two things to know about the numbers. Take them from release builds only:
//! at `opt-level = 0` these codecs run about an order of magnitude slower, which
//! is why the workspace optimises dependencies in the dev profile. And compare
//! like for like on level: `lzma_rust2`'s preset 6 uses a smaller dictionary
//! than 7-Zip's `-mx=6` and lands ~52% larger, whereas level 9 matches.
//!
//! TODO: worth raising upstream with sevenz-rust2. Every item is about moving
//! or relabelling data that is already encoded, not about the codec —
//!
//! * There is no rename API. An entry's name lives in the header, so renaming
//!   ought to need no re-encoding at all; today it costs a full rebuild, 8.86s
//!   against `7z rn`'s 0.05s on the fixture above.
//! * Nor is there a way to copy an already encoded block into a new archive,
//!   which is what would make a delete or an append cheap. Both are rebuilds.
//! * `Lzma2Options::from_level_mt` clamps its chunk size up to the dictionary
//!   size, so at level 9 (64 MB dictionary) anything smaller than that is a
//!   single chunk and threading buys nothing. It gave 1.4x at level 6.

use super::sevenzip::{ArchiveCompression, ArchiveType, tool};
use anyhow::{Context, Result};
use sevenz_rust2::encoder_options::{Lzma2Options, ZstandardOptions};
use sevenz_rust2::{ArchiveEntry, ArchiveReader, ArchiveWriter, EncoderMethod, Password};
use std::fs::File;
use std::io;
use std::path::Path;
use std::str::FromStr;
use tokio::task::spawn_blocking;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

/// Which of the two formats a path holds, taken from its extension exactly as
/// `as_archive` does.
fn archive_type(path: &Path) -> Result<ArchiveType> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_lowercase();
    ArchiveType::from_str(&extension).context("Not a valid archive")
}

/// List the entries of an archive as `(path, size, crc)`, optionally narrowed to
/// a single name.
///
/// The CRC is rendered the way the database stores it: eight lowercase hex
/// digits, matching what `to_hex` produces for a [`crate::crc32::Crc32`] digest.
pub async fn parse<P: AsRef<Path>>(
    path: P,
    name: Option<&str>,
) -> Result<Vec<(String, u64, String)>> {
    let path = path.as_ref().to_path_buf();
    let name = name.map(|name| name.to_string());
    spawn_blocking(move || {
        let wanted = |entry: &str| name.as_ref().is_none_or(|name| name == entry);
        match archive_type(&path)? {
            ArchiveType::Sevenzip => {
                let archive = sevenz_rust2::Archive::open(&path)
                    .with_context(|| format!("Failed to open \"{}\"", path.display()))?;
                Ok(archive
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
                    .collect())
            }
            ArchiveType::Zip => {
                let file = File::open(&path)
                    .with_context(|| format!("Failed to open \"{}\"", path.display()))?;
                let mut archive = ZipArchive::new(file)
                    .with_context(|| format!("Failed to read \"{}\"", path.display()))?;
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

/// Extract a single entry into `directory`, recreating any leading directories
/// the entry name carries.
pub async fn extract<P: AsRef<Path>, Q: AsRef<Path>>(
    path: P,
    name: &str,
    directory: Q,
) -> Result<()> {
    let path = path.as_ref().to_path_buf();
    let name = name.to_string();
    let directory = directory.as_ref().to_path_buf();
    spawn_blocking(move || {
        let destination = directory.join(&name);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match archive_type(&path)? {
            ArchiveType::Sevenzip => {
                let mut reader = ArchiveReader::open(&path, Password::empty())
                    .with_context(|| format!("Failed to open \"{}\"", path.display()))?;
                reader.set_thread_count(threads());
                let data = reader
                    .read_file(&name)
                    .with_context(|| format!("Failed to extract \"{}\"", name))?;
                std::fs::write(&destination, data)?;
            }
            ArchiveType::Zip => {
                let mut archive = ZipArchive::new(File::open(&path)?)
                    .with_context(|| format!("Failed to read \"{}\"", path.display()))?;
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

/// Add `entry_path` (relative to `working_directory`) to an archive, creating it
/// if it does not exist yet.
pub async fn create<P: AsRef<Path>, Q: AsRef<Path>>(
    archive_path: P,
    working_directory: Q,
    entry_path: &Path,
    archive_type: &ArchiveType,
    compression: &ArchiveCompression,
    solid: bool,
) -> Result<()> {
    // Appending to an existing 7z means rewriting it, and `to_archive` is called
    // once per file, so a multi track game would recompress its whole archive
    // for every track it adds. 7-Zip appends a block instead, so it keeps that
    // case — but only when it can read the archive at all, which rules out
    // Zstandard.
    // TODO: batch `to_archive` so all of a game's files are written at once,
    // which would let this be native throughout and produce a better packed
    // archive besides.
    if archive_type == &ArchiveType::Sevenzip
        && archive_path.as_ref().is_file()
        && !compression.is_zstd()
        && !is_zstd(archive_path.as_ref())
    {
        return tool::create(
            archive_path,
            working_directory,
            entry_path,
            archive_type,
            compression,
            solid,
        )
        .await;
    }

    let archive_path = archive_path.as_ref().to_path_buf();
    let working_directory = working_directory.as_ref().to_path_buf();
    let entry_path = entry_path.to_path_buf();
    let archive_type = *archive_type;
    let compression = *compression;
    spawn_blocking(move || {
        let name = entry_path.to_string_lossy().replace('\\', "/");
        let source = working_directory.join(&entry_path);
        match archive_type {
            ArchiveType::Zip => {
                // Zstandard is method 93 in the ZIP spec. Nothing else in the
                // toolchain reads it, so it is only ever written on request.
                let (method, level) = match compression {
                    ArchiveCompression::Zstd(level) => {
                        (CompressionMethod::Zstd, Some(level as i64))
                    }
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
                    ZipWriter::new_append(
                        File::options().read(true).write(true).open(&archive_path)?,
                    )?
                } else {
                    ZipWriter::new(File::create(&archive_path)?)
                };
                writer.start_file(&name, options)?;
                io::copy(&mut File::open(&source)?, &mut writer)?;
                writer.finish()?;
            }
            ArchiveType::Sevenzip => {
                write_sevenzip(
                    &archive_path,
                    vec![(name, std::fs::read(&source)?)],
                    compression,
                    solid,
                )?;
            }
        }
        Ok(())
    })
    .await
    .context("Archive task failed")?
}

/// Rename an entry, rebuilding the archive around it.
///
/// 7z goes to 7-Zip, which relabels the entry in the header. Rebuilding here
/// would re-encode every byte to change a name: measured at 8.86s against
/// 0.05s on a 28MB archive. See the module TODO.
pub async fn rename<P: AsRef<Path>>(path: P, from: &str, to: &str) -> Result<()> {
    if archive_type(path.as_ref())? == ArchiveType::Sevenzip && !is_zstd(path.as_ref()) {
        return tool::rename(path, from, to).await;
    }
    let (from, to) = (from.to_string(), to.to_string());
    rebuild(path, move |name| {
        if name == from {
            Some(to.clone())
        } else {
            Some(name.to_string())
        }
    })
    .await
}

/// Delete an entry, rebuilding the archive around it.
///
/// 7z goes to 7-Zip for the same reason as [`rename`]: it can drop an entry
/// without re-encoding the ones that remain.
pub async fn delete<P: AsRef<Path>>(path: P, name: &str) -> Result<()> {
    if archive_type(path.as_ref())? == ArchiveType::Sevenzip && !is_zstd(path.as_ref()) {
        return tool::delete(path, name).await;
    }
    let name = name.to_string();
    rebuild(path, move |entry| {
        if entry == name {
            None
        } else {
            Some(entry.to_string())
        }
    })
    .await
}

/// Rewrite an archive, passing every entry name through `map`: `None` drops the
/// entry, `Some` keeps it under the returned name.
///
/// ZIP copies each kept entry's compressed stream verbatim, so nothing is
/// re-encoded. 7z has no such path and is fully recompressed, since sevenz-rust2
/// exposes no way to rename an entry or copy an encoded block.
async fn rebuild<P, F>(path: P, map: F) -> Result<()>
where
    P: AsRef<Path>,
    F: Fn(&str) -> Option<String> + Send + 'static,
{
    let path = path.as_ref().to_path_buf();
    spawn_blocking(move || {
        let temporary = path.with_extension("oxyromon-tmp");
        match archive_type(&path)? {
            ArchiveType::Zip => {
                let mut archive = ZipArchive::new(File::open(&path)?)
                    .with_context(|| format!("Failed to read \"{}\"", path.display()))?;
                let mut writer = ZipWriter::new(File::create(&temporary)?);
                for index in 0..archive.len() {
                    let entry = archive.by_index_raw(index)?;
                    match map(entry.name()) {
                        Some(name) if name == entry.name() => writer.raw_copy_file(entry)?,
                        Some(name) => writer.raw_copy_file_rename(entry, name)?,
                        None => {}
                    }
                }
                writer.finish()?;
            }
            ArchiveType::Sevenzip => {
                let solid = sevenz_rust2::Archive::open(&path)?.is_solid;
                // Rewrite it as it was found, so a rename never silently
                // re-encodes a Zstandard archive as LZMA2 or the other way.
                let compression = if is_zstd(&path) {
                    ArchiveCompression::Zstd(DEFAULT_ZSTD_LEVEL)
                } else {
                    ArchiveCompression::Default
                };
                let entries: Vec<(String, Vec<u8>)> = read_sevenzip(&path)?
                    .into_iter()
                    .filter_map(|(name, data)| map(&name).map(|name| (name, data)))
                    .collect();
                if entries.is_empty() {
                    std::fs::remove_file(&path)?;
                    return Ok(());
                }
                write_sevenzip(&temporary, entries, compression, solid)?;
            }
        }
        std::fs::rename(&temporary, &path)?;
        Ok(())
    })
    .await
    .context("Archive task failed")?
}

/// 7-Zip's own default, used when no level is configured.
const DEFAULT_COMPRESSION_LEVEL: usize = 9;

/// What a rebuilt Zstandard archive is re-encoded at when the caller has no
/// level to offer, matching the setting's own default.
const DEFAULT_ZSTD_LEVEL: usize = 19;

/// Threads to hand the LZMA2 codec, which is where the bulk of the time goes.
fn threads() -> u32 {
    std::thread::available_parallelism()
        .map(|available| available.get() as u32)
        .unwrap_or(1)
}

/// True when a 7z archive is Zstandard compressed, which 7-Zip cannot read.
///
/// Answered from the block headers, so it costs a metadata read rather than a
/// decode. An archive that cannot be opened at all is reported as not zstd:
/// whatever is wrong with it is 7-Zip's to report, not ours to guess at.
fn is_zstd(path: &Path) -> bool {
    sevenz_rust2::Archive::open(path)
        .map(|archive| {
            archive.blocks.iter().any(|block| {
                block
                    .coders
                    .iter()
                    .any(|coder| coder.encoder_method_id() == EncoderMethod::ID_ZSTD)
            })
        })
        .unwrap_or(false)
}

/// Read every entry of a 7z archive into memory as `(name, contents)`.
fn read_sevenzip(path: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let mut reader = ArchiveReader::open(path, Password::empty())
        .with_context(|| format!("Failed to open \"{}\"", path.display()))?;
    reader.set_thread_count(threads());
    let mut entries = Vec::new();
    reader.for_each_entries(|entry, reader| {
        if entry.is_directory {
            return Ok(true);
        }
        let mut data = Vec::with_capacity(entry.size as usize);
        io::copy(reader, &mut data)?;
        entries.push((entry.name.clone(), data));
        Ok(true)
    })?;
    Ok(entries)
}

/// Write a 7z archive from entries held in memory.
///
/// The chunk size passed to [`Lzma2Options::from_level_mt`] is clamped up to the
/// dictionary size, so passing 1 asks for the finest split the level allows. A
/// payload smaller than one dictionary is a single chunk and stays effectively
/// single threaded, which is why threading buys nothing on small archives.
fn write_sevenzip(
    path: &Path,
    entries: Vec<(String, Vec<u8>)>,
    compression: ArchiveCompression,
    solid: bool,
) -> Result<()> {
    let mut writer = ArchiveWriter::create(path)
        .with_context(|| format!("Failed to create \"{}\"", path.display()))?;
    writer.set_content_methods(vec![match compression {
        // Zstandard's own encoder is already threaded, and takes the level
        // straight through on its 1-22 scale.
        ArchiveCompression::Zstd(level) => ZstandardOptions::from_level(level as u32).into(),
        ArchiveCompression::Lzma2(level) => {
            Lzma2Options::from_level_mt(level as u32, threads(), 1).into()
        }
        _ => Lzma2Options::from_level_mt(DEFAULT_COMPRESSION_LEVEL as u32, threads(), 1).into(),
    }]);
    if solid {
        writer.push_archive_entries(
            entries
                .iter()
                .map(|(name, _)| ArchiveEntry::new_file(name))
                .collect(),
            entries
                .iter()
                .map(|(_, data)| io::Cursor::new(data.as_slice()).into())
                .collect(),
        )?;
    } else {
        for (name, data) in &entries {
            writer.push_archive_entry(
                ArchiveEntry::new_file(name),
                Some(io::Cursor::new(data.as_slice())),
            )?;
        }
    }
    writer.finish()?;
    Ok(())
}

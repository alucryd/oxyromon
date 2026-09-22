//! Nintendo Switch NSP/NSZ handling backed by the [`nsz_rs`] crate.
//!
//! This replaces the external `nsz` subprocess. Everything the crate does is
//! synchronous and CPU bound, so each entry point hands its work to the blocking
//! pool rather than occupying a runtime worker for the length of a (de)compression.
//!
//! Keys are handed over as a loader that only runs when needed: decompression
//! is unverified so never needs them, and compression only does for NSPs
//! holding NCAs. Homebrew NSPs, for instance, need no `prod.keys` at all.
//!
//! The flag mapping from the old subprocess calls:
//! - `to_nsp` ran `nsz -D -F` → [`decompress_nsz`] with `fix_padding = true`
//!   (re-pad the header to 0x20), `verify = false`, `strict = false`.
//! - `to_nsz` ran `nsz -C -K -L -P` → [`compress_nsp`] with a solid stream
//!   (`block_size_exponent = None`), long-distance matching (`ldm = true`) at the
//!   nsz default level 18, and `fix_padding = false`. The `-K` (keep) behaviour
//!   is inherent to the crate: members it cannot compress are copied verbatim, and
//!   `-P` (always-parse-cnmt) is only needed by the Python tool for metadata.

use super::common::*;
use super::config::*;
use super::mimetype::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use anyhow::{Context, Result, bail};
use indicatif::ProgressBar;
use nsz_rs::keys::Keys;
use nsz_rs::pipeline::{Compression, compress_nsp, decompress_nsz};
use sqlx::SqliteConnection;
use std::io;
use std::path::{Path, PathBuf};

/// zstd level nsz compresses at by default.
const COMPRESSION_LEVEL: i32 = 18;

/// `~/.switch/prod.keys`, where nsz keeps the Switch keys.
fn keys_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".switch").join("prod.keys"))
}

/// Load and derive the Switch key set.
///
/// Keys are CRC-verified, matching the external tool's own sanity check: a
/// tampered `prod.keys` is rejected rather than producing garbage output.
fn load_keys() -> nsz_rs::Result<Keys> {
    let path = keys_path().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no home directory to look for .switch/prod.keys in",
        )
    })?;
    Keys::load(path, true)
}

pub struct NspRomfile {
    pub romfile: CommonRomfile,
}

impl GetRomfile for NspRomfile {
    fn romfile(&self) -> &CommonRomfile {
        &self.romfile
    }
}

pub struct NszRomfile {
    pub romfile: CommonRomfile,
}

impl GetRomfile for NszRomfile {
    fn romfile(&self) -> &CommonRomfile {
        &self.romfile
    }
}

impl HashAndSize for NszRomfile {
    async fn get_hash_and_size(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        position: usize,
        total: usize,
        hash_algorithm: &HashAlgorithm,
    ) -> Result<(String, u64)> {
        let tmp_directory = create_tmp_directory(connection).await?;
        let nsp_romfile = self.to_nsp(progress_bar, &tmp_directory).await?;
        let (hash, size) = nsp_romfile
            .romfile
            .get_hash_and_size(connection, progress_bar, position, total, hash_algorithm)
            .await?;
        nsp_romfile.romfile.delete(progress_bar, true).await?;
        Ok((hash, size))
    }
}

impl Check for NszRomfile {
    async fn check(
        &self,
        connection: &mut SqliteConnection,
        progress_bar: &ProgressBar,
        header: &Option<Header>,
        roms: &[&Rom],
    ) -> Result<()> {
        print_action(progress_bar, &format!("Checking \"{}\"", self.romfile));
        let tmp_directory = create_tmp_directory(connection).await?;
        let nsp_romfile = self.to_nsp(progress_bar, &tmp_directory).await?;
        nsp_romfile
            .romfile
            .check(connection, progress_bar, header, roms)
            .await?;
        Ok(())
    }
}

pub trait ToNsp {
    async fn to_nsp<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<NspRomfile>;
}

impl ToNsp for NszRomfile {
    async fn to_nsp<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<NspRomfile> {
        start_action(progress_bar, Some("Extracting nsz"));

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
            .with_extension(NSP_EXTENSION);
        let (input, output) = (self.romfile.path.clone(), path.clone());
        run_blocking(progress_bar, input.metadata()?.len(), move |progress| {
            decompress_nsz(&input, &output, load_keys, true, false, false, progress)
        })
        .await
        .with_context(|| format!("Failed to decompress \"{}\"", self.romfile.path.display()))?;

        stop_action(progress_bar);

        CommonRomfile::from_path(&path)?.as_nsp()
    }
}

pub trait ToNsz {
    async fn to_nsz<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<NszRomfile>;
}

impl ToNsz for NspRomfile {
    async fn to_nsz<P: AsRef<Path>>(
        &self,
        progress_bar: &ProgressBar,
        destination_directory: &P,
    ) -> Result<NszRomfile> {
        start_action(progress_bar, Some("Creating nsz"));

        let path = destination_directory
            .as_ref()
            .join(self.romfile.path.file_name().unwrap())
            .with_extension(NSZ_EXTENSION);

        print_action(
            progress_bar,
            &format!(
                "Creating \"{}\"",
                path.file_name().unwrap().to_str().unwrap()
            ),
        );

        let (input, output) = (self.romfile.path.clone(), path.clone());
        let compression = Compression {
            level: COMPRESSION_LEVEL,
            ldm: true,
            block_size_exponent: None,
        };
        run_blocking(progress_bar, input.metadata()?.len(), move |progress| {
            compress_nsp(&input, &output, load_keys, &compression, false, progress)
        })
        .await
        .with_context(|| format!("Failed to compress \"{}\"", self.romfile.path.display()))?;

        stop_action(progress_bar);

        CommonRomfile::from_path(&path)?.as_nsz()
    }
}

pub trait AsNsp {
    fn as_nsp(self) -> Result<NspRomfile>;
}

impl AsNsp for CommonRomfile {
    fn as_nsp(self) -> Result<NspRomfile> {
        if self
            .path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase()
            != NSP_EXTENSION
        {
            bail!("Not a valid nsp");
        }
        Ok(NspRomfile { romfile: self })
    }
}

pub trait AsNsz {
    fn as_nsz(self) -> Result<NszRomfile>;
}

impl AsNsz for CommonRomfile {
    fn as_nsz(self) -> Result<NszRomfile> {
        if self
            .path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase()
            != NSZ_EXTENSION
        {
            bail!("Not a valid nsz");
        }
        Ok(NszRomfile { romfile: self })
    }
}

/// Reported by `info`. Like nod, a linked library has no version to query at
/// runtime. It never fails, since only compressing NCAs needs keys, but says
/// when `prod.keys` is missing so that isn't first discovered mid-compression.
pub async fn get_version() -> Result<String> {
    Ok(match keys_path().is_some_and(|path| path.is_file()) {
        true => String::from("built-in"),
        false => String::from("built-in, prod.keys not found"),
    })
}

#[cfg(test)]
mod test_as_nsp_as_nsz;
#[cfg(test)]
mod test_check;
#[cfg(test)]
mod test_check_mismatch;
#[cfg(test)]
mod test_hash_and_size;
#[cfg(test)]
mod test_to_nsp;
#[cfg(test)]
mod test_to_nsz;

//! IPS and BPS patch application, part of oxyROMon.
//!
//! Applies patches as [Flips] does, which this began as a port of: IPS with
//! its truncation extension and Flips' warnings, and BPS with its checks of
//! the patch, the source and the output. An XPS is either.
//!
//! ```no_run
//! use std::path::Path;
//!
//! let (rom, patch) = (Path::new("game.sfc"), Path::new("hack.bps"));
//! xps_rs::apply(rom, patch, Path::new("hack.sfc"), &mut |_| {}).unwrap();
//! ```
//!
//! [Flips]: https://github.com/Alcaro/Flips

mod bps;
mod error;
mod ips;

pub use error::{Error, Result};

use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Ips,
    Bps,
}

/// What Flips warns about while still applying an IPS patch, which carries no
/// checksum to tell a wrong source by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Warning {
    /// It truncates, but the source is no longer than that.
    NotThis,
    /// It changed nothing: the source is its output already.
    AlreadyApplied,
    /// Its records reach past its own truncation.
    Scrambled,
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            Warning::NotThis => "the patch is most likely not intended for this file",
            Warning::AlreadyApplied => {
                "the patch did nothing: this is most likely its output already"
            }
            Warning::Scrambled => "the patch appears scrambled or malformed",
        })
    }
}

/// Which format a patch is, from its magic.
pub fn identify(patch: &Path) -> Result<Format> {
    let mut magic = [0; 5];
    let n = File::open(patch)?.take(5).read(&mut magic)?;
    match &magic[..n] {
        [b'P', b'A', b'T', b'C', b'H'] => Ok(Format::Ips),
        [b'B', b'P', b'S', b'1', ..] => Ok(Format::Bps),
        found => Err(Error::BadMagic {
            expected: "PATCH or BPS1".into(),
            found: String::from_utf8_lossy(found).into_owned(),
        }),
    }
}

/// Apply `patch` to `source`, writing what it produces to `output`.
///
/// Everything is checked before `output` is touched, and it is written as
/// `<output>.part`, moved into place once complete: a failed run leaves an
/// existing `output` alone and nothing behind. `progress` is called with the
/// patch bytes consumed; the calls add up to the patch size.
pub fn apply(
    source: &Path,
    patch: &Path,
    output: &Path,
    progress: &mut dyn FnMut(u64),
) -> Result<Option<Warning>> {
    let format = identify(patch)?;
    let patch_file = File::open(patch)?;
    let mut source = File::open(source)?;
    let part = part(output);
    match format {
        Format::Ips => {
            let bytes = std::fs::read(patch)?;
            let parsed = ips::parse(&bytes)?;
            let result = (|| {
                let warning = ips::apply(parsed, &mut source, &mut create(&part)?)?;
                progress(bytes.len() as u64);
                Ok(warning)
            })();
            finish(&part, output, result)
        }
        Format::Bps => {
            let bps = bps::Bps::open(&patch_file, &source)?;
            let result = (|| bps.apply(&mut create(&part)?, progress).map(|()| None))();
            finish(&part, output, result)
        }
    }
}

fn create(path: &Path) -> Result<File> {
    Ok(File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?)
}

fn part(output: &Path) -> PathBuf {
    let mut name = output.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    output.with_file_name(name)
}

fn finish<T>(part: &Path, output: &Path, result: Result<T>) -> Result<T> {
    let result = result.and_then(|value| {
        std::fs::rename(part, output)?;
        Ok(value)
    });
    if result.is_err() {
        let _ = std::fs::remove_file(part);
    }
    result
}

fn read_at(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
    #[cfg(unix)]
    {
        std::os::unix::fs::FileExt::read_at(file, buf, offset)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::FileExt::seek_read(file, buf, offset)
    }
}

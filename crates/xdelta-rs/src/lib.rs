//! xdelta3 patch decoding, part of oxyROMon.
//!
//! Applies VCDIFF (RFC 3284) patches as [xdelta3] writes them, which this began
//! as a port of: its per-window Adler-32 checks, and its LZMA secondary
//! compression, the default since 3.0. DJW and FGK, which xdelta3 only writes
//! when asked, are refused, as are code tables of the patch's own and windows
//! that copy from the target, which xdelta3 does not decode either.
//!
//! ```no_run
//! use std::path::Path;
//!
//! let (rom, patch) = (Path::new("game.rom"), Path::new("hack.xdelta"));
//! xdelta_rs::decode(Some(rom), patch, Path::new("hack.rom"), &mut |_| {}).unwrap();
//! ```
//!
//! [xdelta3]: https://github.com/jmacd/xdelta

mod error;
mod secondary;
mod vcdiff;

pub use error::{Error, Result};

use secondary::Secondary;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

/// What a patch says about itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Header {
    /// The name of the file it produces, as xdelta3 records it.
    pub target_name: Option<String>,
    /// The name of the file it applies to.
    pub source_name: Option<String>,
}

/// Read a patch's header, checking it is one this crate decodes.
pub fn read_header(patch: &Path) -> Result<Header> {
    let header = vcdiff::read_file_header(&mut BufReader::new(File::open(patch)?))?;
    // xdelta3 writes `target/compression/source/compression`.
    let app_header = String::from_utf8_lossy(&header.app_header);
    let fields: Vec<&str> = app_header.split('/').collect();
    let name = |index: usize| {
        fields
            .get(index)
            .filter(|name| fields.len() >= 3 && !name.is_empty())
            .map(|name| name.to_string())
    };
    Ok(Header {
        target_name: name(0),
        source_name: name(2),
    })
}

/// Apply `patch` to `source`, writing what it produces to `output`.
///
/// `source` may only be `None` for a patch that copies from none. `progress` is
/// called with the patch bytes consumed, window by window; the calls add up to
/// the patch size. `output` is written as `<output>.part` and moved into place
/// once complete, so a failed run leaves an existing `output` alone and nothing
/// behind.
pub fn decode(
    source: Option<&Path>,
    patch: &Path,
    output: &Path,
    progress: &mut dyn FnMut(u64),
) -> Result<()> {
    let mut reader = Counting {
        inner: BufReader::new(File::open(patch)?),
        count: 0,
    };
    let header = vcdiff::read_file_header(&mut reader)?;
    let source = source.map(File::open).transpose()?;

    let part = part(output);
    let result = (|| {
        let mut out = BufWriter::new(File::create(&part)?);
        let mut secondary = header.secondary.map(Secondary::new);
        let mut target = Vec::new();
        let mut reported = 0;
        let mut number = 0;
        while let Some(mut window) = vcdiff::read_window(&mut reader, secondary.is_some())? {
            if let Some(secondary) = &mut secondary {
                secondary.decompress(&mut window)?;
            }
            vcdiff::apply(&window, source.as_ref(), &mut target)?;
            if window
                .adler32
                .is_some_and(|expected| expected != vcdiff::adler32(&target))
            {
                return Err(Error::Corrupt(format!(
                    "window {number} does not match its checksum; is the source the right file?"
                )));
            }
            out.write_all(&target)?;
            progress(reader.count - reported);
            reported = reader.count;
            number += 1;
        }
        out.flush()?;
        Ok(())
    })();
    finish(&part, output, result)
}

struct Counting<R> {
    inner: R,
    count: u64,
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.count += n as u64;
        Ok(n)
    }
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

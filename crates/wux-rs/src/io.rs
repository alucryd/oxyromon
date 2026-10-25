//! Positional reads, so many workers can read one file without seeking, and
//! the `.part` file an output is written to.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::Result;

/// Where `output` is written until it is complete: next to it, so that moving
/// it into place is a rename, and an existing file is only ever replaced by a
/// complete one.
pub fn part(output: &Path) -> PathBuf {
    let mut name = output.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    output.with_file_name(name)
}

/// Move `part` into place at `output` if its write succeeded, and remove it
/// otherwise: a failed run leaves nothing behind, and `output` as it was.
pub fn finish<T>(part: &Path, output: &Path, result: Result<T>) -> Result<T> {
    let result = result.and_then(|value| {
        std::fs::rename(part, output)?;
        Ok(value)
    });
    if result.is_err() {
        let _ = std::fs::remove_file(part);
    }
    result
}

/// Read `buf` from `file` at `offset` without moving any shared file cursor.
pub fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.read_at(buf, offset)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        file.seek_read(buf, offset)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (file, buf, offset);
        Err(io::Error::other(
            "positional reads are not supported on this platform",
        ))
    }
}

/// Like [`read_at`], but errors if the file is too short.
pub fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<()> {
    let mut done = 0;
    while done < buf.len() {
        match read_at(file, &mut buf[done..], offset + done as u64)? {
            0 => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "reached end of file early",
                ));
            }
            n => done += n,
        }
    }
    Ok(())
}

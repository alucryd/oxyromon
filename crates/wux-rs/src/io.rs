//! Positional reads, so many workers can read one file without seeking, and
//! the `.part` file an output is written to, and progress scaled to a budget.

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

/// [`finish`] for a folder. An existing `output` is replaced only if it is
/// an earlier output of the same, every file in it one `part` holds too:
/// anything else is left as it is, and the write fails.
pub fn finish_dir<T>(part: &Path, output: &Path, result: Result<T>) -> Result<T> {
    let result = result.and_then(|value| {
        if output.exists() {
            if !holds_only(output, part) {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!(
                        "{} is in the way, and holds files this would not write",
                        output.display()
                    ),
                )
                .into());
            }
            std::fs::remove_dir_all(output)?;
        }
        std::fs::rename(part, output)?;
        Ok(value)
    });
    if result.is_err() {
        let _ = std::fs::remove_dir_all(part);
    }
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

/// Bytes reported as a share of `budget`, their sum being `total`.
pub(crate) struct Scaled<'a> {
    report: &'a mut dyn FnMut(u64),
    total: u64,
    budget: u64,
    done: u64,
    reported: u64,
}

impl<'a> Scaled<'a> {
    pub(crate) fn new(report: &'a mut dyn FnMut(u64), total: u64, budget: u64) -> Self {
        Scaled {
            report,
            total,
            budget,
            done: 0,
            reported: 0,
        }
    }

    pub(crate) fn add(&mut self, n: u64) {
        self.done = (self.done + n).min(self.total);
        let scaled = (u128::from(self.done) * u128::from(self.budget)
            / u128::from(self.total.max(1))) as u64;
        (self.report)(scaled - self.reported);
        self.reported = scaled;
    }

    /// Report whatever is left of the budget: all of it when nothing was added.
    pub(crate) fn finish(&mut self) {
        (self.report)(self.budget - self.reported);
        self.reported = self.budget;
    }
}

//! Positional reads, so many workers can read one file without seeking.

use std::fs::File;
use std::io;

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

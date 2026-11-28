//! PlayStation 3 discs, part of oxyROMon.
//!
//! Most PS3 games circulate as JB folders, a disc's files decrypted, which
//! with the disc's IRD rebuild the Redump ISO byte for byte: the IRD holds
//! the sectors before the first file and after the last, and the data1 the
//! disc key derives from. The other way, an ISO extracts to its JB folder.
//!
//! ```no_run
//! use std::path::Path;
//!
//! let ird = ps3_rs::Ird::read(Path::new("Game.ird")).unwrap();
//! let folder = Path::new("Game");
//! let md5 = ps3_rs::rebuild(
//!     &ird,
//!     &mut |path| Some(folder.join(path)),
//!     Path::new("Game.iso"),
//!     &mut |_| {},
//! )
//! .unwrap();
//! ```

pub mod disc;
mod error;
mod ird;

pub use disc::{File, Region, SECTOR, disc_key};
pub use error::{Error, Result};
pub use ird::{Ird, MAX_INFLATED};

use std::collections::HashMap;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Component, Path, PathBuf};

use md5::{Digest, Md5};

use disc::Crypt;

/// Sectors read and written at once.
const CHUNK: usize = 512;

/// Where a JB folder keeps the system update the disc installs.
pub const UPDATE_PATH: &str = "PS3_UPDATE/PS3UPDAT.PUP";

/// Rebuild the disc `ird` describes from its files, which `source` finds by
/// their path on the disc, such as `PS3_GAME/USRDIR/EBOOT.BIN`, and return
/// the MD5 of what it wrote.
///
/// Every file is checked against the size and MD5 the IRD gives it, so a
/// wrong one fails the rebuild. Written as `<output>.part`, moved into place
/// once complete: a failed run leaves an existing `output` alone and nothing
/// behind. `progress` is called with the file bytes read; the calls add up
/// to the files' size.
pub fn rebuild(
    ird: &Ird,
    source: &mut dyn FnMut(&str) -> Option<PathBuf>,
    output: &Path,
    progress: &mut dyn FnMut(u64),
) -> Result<[u8; 16]> {
    let size = ird.size();
    let total = (size / SECTOR as u64) as u32;
    let header_sectors = (ird.header.len() / SECTOR) as u32;
    if !ird.footer.len().is_multiple_of(SECTOR)
        || (ird.header.len() + ird.footer.len()) as u64 > size
    {
        return Err(Error::Corrupt(format!(
            "an IRD footer of {} bytes on a disc of {size}",
            ird.footer.len()
        )));
    }
    let footer_start = total - (ird.footer.len() / SECTOR) as u32;
    let crypt = Crypt::new(&ird.disc_key(), disc::regions(&ird.header[..SECTOR])?);
    let hashes: HashMap<u64, [u8; 16]> = ird.file_hashes.iter().copied().collect();

    // Every file found, the right size and readable before anything is
    // written, but held open only as it is read: a disc can hold thousands,
    // past the limit of open files
    let files = ird.files()?;
    let mut sources = Vec::with_capacity(files.len());
    for file in &files {
        let path =
            source(&file.path).ok_or_else(|| Error::WrongSource(format!("no {}", file.path)))?;
        let len = fs::metadata(&path)
            .map_err(|e| Error::WrongSource(format!("{}: {e}", path.display())))?
            .len();
        if len != file.size() {
            return Err(Error::WrongSource(format!(
                "{} is {len} bytes, expected {}",
                file.path,
                file.size()
            )));
        }
        fs::File::open(&path)
            .map_err(|e| Error::WrongSource(format!("{}: {e}", path.display())))?;
        sources.push(path);
    }

    // Each extent where it lies, in order, with where it starts in its file
    let mut extents: Vec<(u32, u32, usize, u64)> = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let mut offset = 0;
        for &(lba, len) in &file.extents {
            if len > 0 {
                extents.push((lba, len, index, offset));
            }
            offset += len as u64;
        }
    }
    extents.sort_by_key(|extent| extent.0);

    // What lies under no file: the header, the footer, or zeros
    let base = |lba: u32, sector: &mut [u8]| {
        let bytes = if lba >= footer_start {
            let at = (lba - footer_start) as usize * SECTOR;
            Some(&ird.footer[at..at + SECTOR])
        } else if lba < header_sectors {
            let at = lba as usize * SECTOR;
            Some(&ird.header[at..at + SECTOR])
        } else {
            None
        };
        match bytes {
            Some(bytes) => sector.copy_from_slice(bytes),
            None => sector.fill(0),
        }
    };

    let part = part(output);
    let result = (|| {
        let mut out = HashingWriter {
            inner: BufWriter::with_capacity(CHUNK * SECTOR, create(&part)?),
            md5: Md5::new(),
        };
        let mut buf = vec![0u8; CHUNK * SECTOR];
        let mut file_md5s: Vec<Md5> = vec![Md5::new(); files.len()];
        // the bytes of each file hashed so far, which its extents must continue
        let mut hashed: Vec<u64> = vec![0; files.len()];
        let mut lba = 0u32;
        let mut fill = |out: &mut HashingWriter<_>, from: u32, to: u32| -> Result<()> {
            let mut at = from;
            while at < to {
                let n = (to - at).min(CHUNK as u32) as usize;
                for (i, sector) in buf[..n * SECTOR]
                    .as_chunks_mut::<SECTOR>()
                    .0
                    .iter_mut()
                    .enumerate()
                {
                    base(at + i as u32, sector);
                }
                crypt.encrypt(at, &mut buf[..n * SECTOR]);
                out.write_all(&buf[..n * SECTOR])?;
                at += n as u32;
            }
            Ok(())
        };
        let mut chunk = vec![0u8; CHUNK * SECTOR];
        // the one file open, by its index: extents come in disc order, a
        // file's together but for one past 4 GiB
        let mut open: Option<(usize, fs::File)> = None;
        for &(start, len, index, offset) in &extents {
            let end = start + len.div_ceil(SECTOR as u32);
            if start < lba || end > total {
                return Err(Error::Corrupt(format!(
                    "{} at sectors {start}-{end}, overlapping another file or past the disc",
                    files[index].path
                )));
            }
            if offset != hashed[index] {
                return Err(Error::Unsupported(format!(
                    "{} with its extents out of order",
                    files[index].path
                )));
            }
            fill(&mut out, lba, start)?;
            if open.as_ref().is_none_or(|(at, _)| *at != index) {
                open = Some((index, fs::File::open(&sources[index])?));
            }
            let file = &open.as_ref().unwrap().1;
            let mut done = 0u64;
            while done < len as u64 {
                let n = (len as u64 - done).min(chunk.len() as u64) as usize;
                read_exact_at(file, &mut chunk[..n], offset + done)?;
                file_md5s[index].update(&chunk[..n]);
                progress(n as u64);
                let sectors = n.div_ceil(SECTOR);
                chunk[n..sectors * SECTOR].fill(0);
                let at = start + (done / SECTOR as u64) as u32;
                // the footer wins over a file it overlaps, as an IRD's tools lay it last
                for i in 0..sectors {
                    if at + i as u32 >= footer_start {
                        base(at + i as u32, &mut chunk[i * SECTOR..(i + 1) * SECTOR]);
                    }
                }
                crypt.encrypt(at, &mut chunk[..sectors * SECTOR]);
                out.write_all(&chunk[..sectors * SECTOR])?;
                done += n as u64;
            }
            hashed[index] += len as u64;
            lba = end;
        }
        fill(&mut out, lba, total)?;

        for (index, file) in files.iter().enumerate() {
            let actual: [u8; 16] = file_md5s[index].clone().finalize().into();
            if let Some(expected) = hashes.get(&(file.lba() as u64))
                && *expected != actual
            {
                return Err(Error::WrongSource(format!(
                    "{} has MD5 {}, expected {}",
                    file.path,
                    hex(&actual),
                    hex(expected)
                )));
            }
        }
        out.inner.flush()?;
        Ok(out.md5.finalize().into())
    })();
    finish(&part, output, result)
}

/// Extract the disc `iso` into the JB folder `output`, decrypting it with
/// `key`, and return its files with the MD5 of each.
///
/// Written as `<output>.part`, moved into place once complete, replacing an
/// existing `output` only then. `progress` is called with the disc bytes
/// read; the calls add up to its size.
pub fn extract(
    iso: &Path,
    key: &[u8; 16],
    output: &Path,
    progress: &mut dyn FnMut(u64),
) -> Result<Vec<(File, [u8; 16])>> {
    let input = fs::File::open(iso)?;
    let size = input.metadata()?.len();
    let mut sector0 = vec![0u8; SECTOR];
    read_exact_at(&input, &mut sector0, 0)?;
    let crypt = Crypt::new(key, disc::regions(&sector0)?);
    let files = disc::files(&mut |lba, sector| {
        read_exact_at(&input, sector, lba as u64 * SECTOR as u64)?;
        crypt.decrypt(lba, sector);
        Ok(())
    })?;

    let part = part(output);
    let result = (|| {
        if part.exists() {
            fs::remove_dir_all(&part)?;
        }
        fs::create_dir_all(&part)?;
        let mut read = 0u64;
        let mut chunk = vec![0u8; CHUNK * SECTOR];
        let mut extracted = Vec::with_capacity(files.len());
        for file in files {
            let path = part.join(safe(&file.path)?);
            fs::create_dir_all(path.parent().unwrap())?;
            let mut out = BufWriter::new(create(&path)?);
            let mut md5 = Md5::new();
            for &(lba, len) in &file.extents {
                let mut done = 0u64;
                while done < len as u64 {
                    let n = (len as u64 - done).min(chunk.len() as u64) as usize;
                    let sectors = n.div_ceil(SECTOR);
                    let at = lba as u64 + done / SECTOR as u64;
                    if (at + sectors as u64) * SECTOR as u64 > size {
                        return Err(Error::Corrupt(format!("{} past the disc", file.path)));
                    }
                    read_exact_at(&input, &mut chunk[..sectors * SECTOR], at * SECTOR as u64)?;
                    crypt.decrypt(at as u32, &mut chunk[..sectors * SECTOR]);
                    out.write_all(&chunk[..n])?;
                    md5.update(&chunk[..n]);
                    let consumed = (sectors * SECTOR) as u64;
                    progress(consumed);
                    read += consumed;
                    done += n as u64;
                }
            }
            out.flush()?;
            extracted.push((file, md5.finalize().into()));
        }
        progress(size.saturating_sub(read));
        Ok(extracted)
    })();
    let result = result.and_then(|files| {
        if output.exists() {
            fs::remove_dir_all(output)?;
        }
        fs::rename(&part, output)?;
        Ok(files)
    });
    if result.is_err() {
        let _ = fs::remove_dir_all(&part);
    }
    result
}

/// The firmware version a PS3UPDAT.PUP installs, such as `4.90`, from the
/// `version.txt` it carries.
pub fn pup_version(path: &Path) -> Result<String> {
    let file = fs::File::open(path)?;
    let mut header = [0u8; 0x30];
    read_exact_at(&file, &mut header, 0)?;
    if &header[..8] != b"SCEUF\0\0\0" {
        return Err(Error::BadMagic {
            expected: "SCEUF".into(),
            found: String::from_utf8_lossy(&header[..5]).into_owned(),
        });
    }
    let be = |bytes: &[u8], at: usize| u64::from_be_bytes(bytes[at..at + 8].try_into().unwrap());
    let count = be(&header, 0x18).min(256);
    for i in 0..count {
        let mut entry = [0u8; 0x20];
        read_exact_at(&file, &mut entry, 0x30 + i * 0x20)?;
        // version.txt is entry 0x100
        if be(&entry, 0) == 0x100 {
            let len = be(&entry, 0x10).min(64) as usize;
            let mut text = vec![0u8; len];
            read_exact_at(&file, &mut text, be(&entry, 8))?;
            return Ok(String::from_utf8_lossy(&text).trim().to_string());
        }
    }
    Err(Error::Corrupt("a PUP without version.txt".into()))
}

/// A disc path as a relative one, refusing any that would climb out.
fn safe(path: &str) -> Result<PathBuf> {
    let path = Path::new(path);
    if path
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        Ok(path.to_path_buf())
    } else {
        Err(Error::Corrupt(format!("a file named {}", path.display())))
    }
}

struct HashingWriter<W> {
    inner: W,
    md5: Md5,
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.md5.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn create(path: &Path) -> Result<fs::File> {
    Ok(fs::File::create(path)?)
}

fn part(output: &Path) -> PathBuf {
    let mut name = output.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    output.with_file_name(name)
}

fn finish<T>(part: &Path, output: &Path, result: Result<T>) -> Result<T> {
    let result = result.and_then(|value| {
        fs::rename(part, output)?;
        Ok(value)
    });
    if result.is_err() {
        let _ = fs::remove_file(part);
    }
    result
}

fn read_exact_at(file: &fs::File, buf: &mut [u8], offset: u64) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::FileExt::read_exact_at(file, buf, offset)
    }
    #[cfg(windows)]
    {
        let mut done = 0;
        while done < buf.len() {
            match std::os::windows::fs::FileExt::seek_read(
                file,
                &mut buf[done..],
                offset + done as u64,
            )? {
                0 => return Err(std::io::ErrorKind::UnexpectedEof.into()),
                n => done += n,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data1_derives_the_disc_key_redump_publishes() {
        // Disgaea 3 - Absence of Justice (USA), BLUS-30181: its IRD's data1
        // and Redump's disc key
        let data1 = [
            0xf7, 0xee, 0xee, 0x7a, 0xdd, 0x80, 0xfc, 0xde, 0x23, 0x1d, 0x0c, 0x35, 0xb4, 0xe7,
            0x3b, 0xd7,
        ];
        let dkey = [
            0x73, 0x81, 0x9c, 0xef, 0xe0, 0x85, 0xb2, 0xc8, 0xfe, 0x8a, 0x92, 0xe7, 0xf8, 0x1e,
            0x84, 0xbc,
        ];
        assert_eq!(disc_key(&data1), dkey);
    }

    #[test]
    fn disc_paths_cannot_climb_out() {
        assert!(safe("PS3_GAME/USRDIR/EBOOT.BIN").is_ok());
        assert!(safe("../evil").is_err());
        assert!(safe("/etc/passwd").is_err());
    }
}

//! A disc image, read the same way whether it is a raw WUD or a WUX.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use crate::error::{Error, Result, corrupt};
use crate::io::read_exact_at;
use crate::wux::{HEADER_SIZE, Header, MAGIC};

/// A WUD or WUX, opened for reading its uncompressed bytes. It implements
/// [`Read`] and [`Seek`] over those bytes, and [`Image::read_exact_at`] reads
/// without moving its cursor.
pub struct Image {
    file: File,
    wux: Option<Wux>,
    size: u64,
    position: u64,
}

struct Wux {
    header: Header,
    /// Which stored sector each sector of the disc is.
    table: Vec<u32>,
    data_offset: u64,
}

impl Image {
    /// Open `path`, a WUX if it starts with the WUX magic, a WUD otherwise.
    pub fn open(path: &Path) -> Result<Image> {
        let file = File::open(path)?;
        let file_size = file.metadata()?.len();
        let mut magic = [0; HEADER_SIZE as usize];
        let is_wux = file_size >= HEADER_SIZE && {
            read_exact_at(&file, &mut magic, 0)?;
            magic[..8] == MAGIC
        };
        if !is_wux {
            return Ok(Image {
                file,
                wux: None,
                size: file_size,
                position: 0,
            });
        }

        let header = Header::parse(&magic)?;
        let count = usize::try_from(header.sector_count())
            .map_err(|_| corrupt("the WUX has too many sectors"))?;
        let data_offset = header.data_offset();
        if data_offset > file_size {
            return Err(corrupt("the WUX ends inside its sector table"));
        }
        let mut bytes = vec![0; count * 4];
        read_exact_at(&file, &mut bytes, HEADER_SIZE)?;
        let table: Vec<u32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes(*b))
            .collect();
        let stored = (file_size - data_offset).div_ceil(u64::from(header.sector_size));
        if let Some(index) = table.iter().find(|&&index| u64::from(index) >= stored) {
            return Err(corrupt(format!(
                "the WUX refers to sector {index}, but stores {stored}"
            )));
        }
        Ok(Image {
            file,
            size: header.uncompressed_size,
            wux: Some(Wux {
                header,
                table,
                data_offset,
            }),
            position: 0,
        })
    }

    /// The uncompressed size, that of the WUD.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// The WUX header, `None` for a WUD.
    pub fn header(&self) -> Option<&Header> {
        self.wux.as_ref().map(|wux| &wux.header)
    }

    /// Fill `buf` from uncompressed `offset`, failing past the end.
    pub fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> Result<()> {
        if offset
            .checked_add(buf.len() as u64)
            .is_none_or(|end| end > self.size)
        {
            return Err(Error::Io(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "read past the end of the disc image",
            )));
        }
        let Some(wux) = &self.wux else {
            return Ok(read_exact_at(&self.file, buf, offset)?);
        };
        let sector_size = u64::from(wux.header.sector_size);
        let mut done = 0;
        while done < buf.len() {
            let at = offset + done as u64;
            let within = at % sector_size;
            let take = ((sector_size - within) as usize).min(buf.len() - done);
            let stored = u64::from(wux.table[(at / sector_size) as usize]);
            read_exact_at(
                &self.file,
                &mut buf[done..done + take],
                wux.data_offset + stored * sector_size + within,
            )?;
            done += take;
        }
        Ok(())
    }

    /// Which stored sector each sector of the disc is, `None` for a WUD.
    pub(crate) fn table(&self) -> Option<&[u32]> {
        self.wux.as_ref().map(|wux| wux.table.as_slice())
    }
}

impl Read for Image {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = (self.size.saturating_sub(self.position)).min(buf.len() as u64) as usize;
        self.read_exact_at(&mut buf[..n], self.position)
            .map_err(|e| match e {
                Error::Io(e) => e,
                e => io::Error::other(e),
            })?;
        self.position += n as u64;
        Ok(n)
    }
}

impl Seek for Image {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let position = match pos {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::End(n) => self.size.checked_add_signed(n),
            SeekFrom::Current(n) => self.position.checked_add_signed(n),
        };
        self.position = position
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before the start"))?;
        Ok(self.position)
    }
}

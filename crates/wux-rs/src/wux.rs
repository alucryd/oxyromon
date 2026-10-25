//! The WUX container, as wudd writes it and Cemu reads it.
//!
//! A 32-byte little-endian header, then one little-endian `u32` per sector of
//! the disc naming the stored sector that holds its bytes, then, aligned to a
//! sector, every distinct sector once, in the order the disc first uses it.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::Stats;
use crate::error::{Error, Result, corrupt};
use crate::image::Image;
use crate::io::{finish, part};

/// "WUX0" then `0x1099d02e`, both little-endian.
pub const MAGIC: [u8; 8] = [b'W', b'U', b'X', b'0', 0x2e, 0xd0, 0x99, 0x10];
/// The header's size, where the sector table starts.
pub const HEADER_SIZE: u64 = 32;
/// The sector size wudd writes, a Wii U disc's own.
pub const SECTOR_SIZE: u32 = 0x8000;

/// A WUX header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub sector_size: u32,
    pub uncompressed_size: u64,
    pub flags: u32,
}

impl Header {
    pub(crate) fn parse(bytes: &[u8; HEADER_SIZE as usize]) -> Result<Header> {
        if bytes[..8] != MAGIC {
            return Err(Error::BadMagic {
                expected: "WUX0".into(),
                found: String::from_utf8_lossy(&bytes[..4]).into_owned(),
            });
        }
        let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let header = Header {
            sector_size: u32_at(8),
            uncompressed_size: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
            flags: u32_at(24),
        };
        // Cemu's bounds.
        if !(0x100..0x1000_0000).contains(&header.sector_size) {
            return Err(corrupt(format!(
                "the WUX sector size is {}",
                header.sector_size
            )));
        }
        Ok(header)
    }

    fn to_bytes(self) -> [u8; HEADER_SIZE as usize] {
        let mut bytes = [0; HEADER_SIZE as usize];
        bytes[..8].copy_from_slice(&MAGIC);
        bytes[8..12].copy_from_slice(&self.sector_size.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.uncompressed_size.to_le_bytes());
        bytes[24..28].copy_from_slice(&self.flags.to_le_bytes());
        bytes
    }

    /// How many sectors the disc has, the last one possibly short.
    pub fn sector_count(&self) -> u64 {
        self.uncompressed_size.div_ceil(u64::from(self.sector_size))
    }

    /// Where the stored sectors start: past the table, aligned to a sector.
    pub fn data_offset(&self) -> u64 {
        (HEADER_SIZE + self.sector_count() * 4).next_multiple_of(u64::from(self.sector_size))
    }
}

/// Compress the WUD at `input` into a WUX at `output`, storing each distinct
/// sector once. `progress` is called with input bytes as they are read.
pub fn compress(input: &Path, output: &Path, progress: &mut dyn FnMut(u64)) -> Result<Stats> {
    let file = File::open(input)?;
    let input_size = file.metadata()?.len();
    let mut reader = BufReader::with_capacity(1 << 20, file);
    let mut magic = [0; 8];
    if read_full(&mut reader, &mut magic)? == 8 && magic == MAGIC {
        return Err(Error::Unsupported("the input is already a WUX".into()));
    }
    reader.seek(SeekFrom::Start(0))?;

    let header = Header {
        sector_size: SECTOR_SIZE,
        uncompressed_size: input_size,
        flags: 0,
    };
    let part = part(output);
    let result = (|| {
        let mut writer = BufWriter::with_capacity(1 << 20, File::create(&part)?);
        writer.write_all(&header.to_bytes())?;
        // The table is written once known; reserve its place and the padding.
        let data_offset = header.data_offset();
        std::io::copy(
            &mut std::io::repeat(0).take(data_offset - HEADER_SIZE),
            &mut writer,
        )?;

        let mut table = Vec::with_capacity(header.sector_count() as usize);
        let mut stored: HashMap<[u8; 32], u32> = HashMap::new();
        let mut sector = vec![0; SECTOR_SIZE as usize];
        loop {
            let n = read_full(&mut reader, &mut sector)?;
            if n == 0 {
                break;
            }
            // A short last sector is stored padded, and cut back on the way out.
            sector[n..].fill(0);
            let next = u32::try_from(stored.len())
                .map_err(|_| Error::Unsupported("the WUD is too large".into()))?;
            let index = *stored.entry(Sha256::digest(&sector).into()).or_insert(next);
            if index == next {
                writer.write_all(&sector)?;
            }
            table.push(index);
            progress(n as u64);
        }

        let mut file = writer.into_inner().map_err(|e| e.into_error())?;
        let bytes: Vec<u8> = table.iter().flat_map(|i| i.to_le_bytes()).collect();
        file.seek(SeekFrom::Start(HEADER_SIZE))?;
        file.write_all(&bytes)?;
        let output_size = file.seek(SeekFrom::End(0))?;
        file.sync_all()?;
        Ok(Stats {
            input_size,
            output_size,
        })
    })();
    finish(&part, output, result)
}

/// Decompress the WUX at `input` back into the WUD at `output`. `progress` is
/// called with input bytes as they are first read.
pub fn decompress(input: &Path, output: &Path, progress: &mut dyn FnMut(u64)) -> Result<Stats> {
    let image = Image::open(input)?;
    let (Some(header), Some(table)) = (image.header().copied(), image.table()) else {
        return Err(Error::BadMagic {
            expected: "WUX0".into(),
            found: "a WUD".into(),
        });
    };
    let input_size = std::fs::metadata(input)?.len();
    let sector_size = u64::from(header.sector_size);
    let part = part(output);
    let result = (|| {
        let mut writer = BufWriter::with_capacity(1 << 20, File::create(&part)?);
        let mut sector = vec![0; sector_size as usize];
        let mut cached = None;
        let mut reported = header.data_offset().min(input_size);
        progress(reported);
        // Stored sectors come in the order the disc first uses them, so the
        // first use of each is the input read through once.
        let mut next_new = 0;
        for (i, &index) in table.iter().enumerate() {
            let offset = i as u64 * sector_size;
            let len = sector_size.min(header.uncompressed_size - offset) as usize;
            if cached != Some(index) {
                image.read_exact_at(&mut sector[..len], offset)?;
                cached = (len == sector.len()).then_some(index);
            }
            writer.write_all(&sector[..len])?;
            if index >= next_new {
                let n = (u64::from(index + 1 - next_new) * sector_size).min(input_size - reported);
                reported += n;
                progress(n);
                next_new = index + 1;
            }
        }
        progress(input_size - reported);
        let file = writer.into_inner().map_err(|e| e.into_error())?;
        file.sync_all()?;
        Ok(Stats {
            input_size,
            output_size: header.uncompressed_size,
        })
    })();
    finish(&part, output, result)
}

/// Read until `buf` is full or the input ends, returning how much was read.
pub(crate) fn read_full(reader: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut done = 0;
    while done < buf.len() {
        match reader.read(&mut buf[done..])? {
            0 => break,
            n => done += n,
        }
    }
    Ok(done)
}

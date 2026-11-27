//! IRD files, as ps3-disc-dumper and 3k3y's tools write them: a disc's
//! identity, the sectors before its first file and after its last, every
//! file's MD5 by the sector it starts at, and the data1 its key derives from.

use std::io::Read;
use std::path::Path;

use flate2::read::GzDecoder;

use crate::disc::{self, File, SECTOR};
use crate::{Error, Result};

const MAGIC: &[u8; 4] = b"3IRD";
const PIC_SIZE: usize = 115;

/// An IRD's contents.
#[derive(Clone, Debug)]
pub struct Ird {
    pub version: u8,
    pub game_id: String,
    pub game_name: String,
    /// The firmware the disc's PS3UPDAT.PUP installs, such as `2.35`.
    pub update_version: String,
    pub game_version: String,
    pub app_version: String,
    /// The disc from its first byte, through its file system's directories.
    pub header: Vec<u8>,
    /// The disc's last bytes, after its last file.
    pub footer: Vec<u8>,
    pub region_hashes: Vec<[u8; 16]>,
    /// Each file's MD5, by the sector it starts at.
    pub file_hashes: Vec<(u64, [u8; 16])>,
    pub pic: Vec<u8>,
    pub data1: [u8; 16],
    pub data2: [u8; 16],
    pub uid: u32,
}

/// Reads an IRD's fields in order, failing on a short one.
struct Fields<'a>(&'a [u8]);

impl<'a> Fields<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.0.len() < n {
            return Err(Error::Corrupt("IRD ends early".into()));
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap())
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn text(&mut self, n: usize) -> Result<String> {
        Ok(String::from_utf8_lossy(self.take(n)?)
            .trim_end_matches(['\0', ' '])
            .to_string())
    }

    /// .NET's 7-bit encoded length, which prefixes the name.
    fn length(&mut self) -> Result<usize> {
        let mut value = 0usize;
        for shift in (0..35).step_by(7) {
            let byte = self.take(1)?[0];
            value |= ((byte & 0x7f) as usize) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(Error::Corrupt("IRD name length".into()))
    }

    fn gzipped(&mut self) -> Result<Vec<u8>> {
        let len = self.u32()? as usize;
        let mut out = Vec::new();
        GzDecoder::new(self.take(len)?).read_to_end(&mut out)?;
        Ok(out)
    }
}

impl Ird {
    pub fn read(path: &Path) -> Result<Ird> {
        Ird::parse(&std::fs::read(path)?)
    }

    /// Parse an IRD, gzipped as they are shared or not.
    pub fn parse(bytes: &[u8]) -> Result<Ird> {
        let mut unzipped = Vec::new();
        let bytes = if bytes.starts_with(&[0x1f, 0x8b]) {
            GzDecoder::new(bytes).read_to_end(&mut unzipped)?;
            &unzipped[..]
        } else {
            bytes
        };
        let mut fields = Fields(bytes);
        let magic = fields.take(4)?;
        if magic != MAGIC {
            return Err(Error::BadMagic {
                expected: "3IRD".into(),
                found: String::from_utf8_lossy(magic).into_owned(),
            });
        }
        let version = fields.take(1)?[0];
        // shortcut: version 9 only, the one checked against real IRDs and the
        // one oxyromon accepted; add 6 to 8 if one turns up
        if version != 9 {
            return Err(Error::Unsupported(format!("IRD version {version}")));
        }
        let game_id = fields.text(9)?;
        let name_len = fields.length()?;
        let game_name = fields.text(name_len)?;
        let update_version = fields.text(4)?;
        let game_version = fields.text(5)?;
        let app_version = fields.text(5)?;
        let header = fields.gzipped()?;
        let footer = fields.gzipped()?;
        let regions = fields.take(1)?[0] as usize;
        let region_hashes = (0..regions)
            .map(|_| fields.array())
            .collect::<Result<_>>()?;
        let files = fields.u32()? as usize;
        let file_hashes = (0..files)
            .map(|_| Ok((u64::from_le_bytes(fields.array()?), fields.array()?)))
            .collect::<Result<_>>()?;
        // the extra configuration and the attachment count, both u16
        fields.take(4)?;
        let pic = fields.take(PIC_SIZE)?.to_vec();
        let data1 = fields.array()?;
        let data2 = fields.array()?;
        let uid = fields.u32()?;
        let crc = fields.u32()?;
        let actual = crc32fast::hash(&bytes[..bytes.len() - fields.0.len() - 4]);
        if crc != actual {
            return Err(Error::Corrupt(format!(
                "IRD CRC32 {actual:08x}, expected {crc:08x}"
            )));
        }
        if header.len() < 17 * SECTOR || header.len() % SECTOR != 0 {
            return Err(Error::Corrupt(format!(
                "IRD header of {} bytes",
                header.len()
            )));
        }
        Ok(Ird {
            version,
            game_id,
            game_name,
            update_version,
            game_version,
            app_version,
            header,
            footer,
            region_hashes,
            file_hashes,
            pic,
            data1,
            data2,
            uid,
        })
    }

    /// The disc key its data1 derives.
    pub fn disc_key(&self) -> [u8; 16] {
        disc::disc_key(&self.data1)
    }

    /// The disc's size in bytes.
    pub fn size(&self) -> u64 {
        disc::volume_sectors(&self.header[16 * SECTOR..17 * SECTOR]) as u64 * SECTOR as u64
    }

    /// The disc's files, read from the directories its header holds.
    pub fn files(&self) -> Result<Vec<File>> {
        disc::files(&mut |lba, sector| {
            let at = lba as usize * SECTOR;
            let bytes = self.header.get(at..at + SECTOR).ok_or_else(|| {
                Error::Corrupt(format!("a directory in sector {lba}, past the IRD header"))
            })?;
            sector.copy_from_slice(bytes);
            Ok(())
        })
    }

    /// The MD5 the IRD gives the file starting at `lba`.
    pub fn file_hash(&self, lba: u32) -> Option<[u8; 16]> {
        self.file_hashes
            .iter()
            .find(|(sector, _)| *sector == lba as u64)
            .map(|(_, hash)| *hash)
    }
}

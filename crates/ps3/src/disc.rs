// SPDX-License-Identifier: MIT OR Apache-2.0
//! A PlayStation 3 disc as Redump dumps it: 2048-byte sectors, the regions
//! sector 0 lists left plain and the ones between them encrypted, and an ISO
//! 9660 file system with Joliet names.

use std::collections::HashSet;
use std::io::{Error, ErrorKind, Result};

use aes::Aes128;
use aes::cipher::{Array, BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};

fn invalid(what: String) -> Error {
    Error::new(ErrorKind::InvalidData, what)
}

pub const SECTOR: usize = 2048;

/// The secret and IV that encrypt an IRD's data1 into the disc key.
const DATA1_KEY: [u8; 16] = [
    0x38, 0x0b, 0xcf, 0x0b, 0x53, 0x45, 0x5b, 0x3c, 0x78, 0x17, 0xab, 0x4f, 0xa3, 0xba, 0x90, 0xed,
];
const DATA1_IV: [u8; 16] = [
    0x69, 0x47, 0x47, 0x72, 0xaf, 0x6f, 0xda, 0xb3, 0x42, 0x74, 0x3a, 0xef, 0xaa, 0x18, 0x62, 0x87,
];

/// The disc key, the one Redump publishes as a `.dkey`, from the data1 an
/// IRD carries.
pub fn disc_key(data1: &[u8; 16]) -> [u8; 16] {
    let mut key = *data1;
    cbc_encrypt(&DATA1_KEY, DATA1_IV, &mut key);
    key
}

/// A run of sectors, `start` to `end` inclusive, as sector 0 counts them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub start: u32,
    pub end: u32,
    pub encrypted: bool,
}

/// The regions sector 0 lays out: the plain ones it lists, and the encrypted
/// ones between them.
pub fn regions(sector0: &[u8]) -> Result<Vec<Region>> {
    if sector0.len() < SECTOR {
        return Err(invalid("sector 0 is short".into()));
    }
    let be = |at: usize| u32::from_be_bytes(sector0[at..at + 4].try_into().unwrap());
    let count = be(0) as usize;
    if count == 0 || 8 + 8 * count > SECTOR {
        return Err(invalid(format!("{count} plain regions in sector 0")));
    }
    let mut regions = Vec::with_capacity(count * 2 - 1);
    for i in 0..count {
        let (start, end) = (be(8 + 8 * i), be(12 + 8 * i));
        let next = regions.last().map_or(0, |last: &Region| last.end + 1);
        if start < next || end < start {
            return Err(invalid(format!(
                "plain region {start}-{end} out of order in sector 0"
            )));
        }
        if start > next {
            regions.push(Region {
                start: next,
                end: start - 1,
                encrypted: true,
            });
        }
        regions.push(Region {
            start,
            end,
            encrypted: false,
        });
    }
    Ok(regions)
}

/// Each sector's encryption: AES-128-CBC with the disc key, its IV the
/// sector's number, big-endian, in the encrypted regions; none elsewhere.
pub struct Crypt {
    cipher: Aes128,
    regions: Vec<Region>,
}

impl Crypt {
    pub fn new(key: &[u8; 16], regions: Vec<Region>) -> Crypt {
        Crypt {
            cipher: Aes128::new(key.into()),
            regions,
        }
    }

    /// Whether sector `lba` is encrypted.
    pub fn encrypted(&self, lba: u32) -> bool {
        self.regions
            .iter()
            .any(|region| region.encrypted && (region.start..=region.end).contains(&lba))
    }

    /// Encrypt the whole sectors of `buf`, the first of them `lba`.
    pub fn encrypt(&self, lba: u32, buf: &mut [u8]) {
        for (i, sector) in buf.as_chunks_mut::<SECTOR>().0.iter_mut().enumerate() {
            let lba = lba + i as u32;
            if self.encrypted(lba) {
                let mut chain = iv(lba);
                for block in sector.as_chunks_mut::<16>().0 {
                    for (b, c) in block.iter_mut().zip(chain) {
                        *b ^= c;
                    }
                    self.cipher.encrypt_block(block.into());
                    chain = *block;
                }
            }
        }
    }

    /// Decrypt the whole sectors of `buf`, the first of them `lba`.
    pub fn decrypt(&self, lba: u32, buf: &mut [u8]) {
        let mut blocks = [Array::<u8, _>::default(); SECTOR / 16];
        for (i, sector) in buf.as_chunks_mut::<SECTOR>().0.iter_mut().enumerate() {
            let lba = lba + i as u32;
            if !self.encrypted(lba) {
                continue;
            }
            for (block, bytes) in blocks.iter_mut().zip(sector.as_chunks::<16>().0) {
                block.copy_from_slice(bytes);
            }
            // a sector's blocks decrypt together, which lets AES-NI pipeline them
            self.cipher.decrypt_blocks(&mut blocks);
            let mut chain = iv(lba);
            for (block, bytes) in blocks.iter().zip(sector.as_chunks_mut::<16>().0) {
                let next = *bytes;
                for ((byte, plain), c) in bytes.iter_mut().zip(block.iter()).zip(chain) {
                    *byte = plain ^ c;
                }
                chain = next;
            }
        }
    }
}

fn iv(lba: u32) -> [u8; 16] {
    (lba as u128).to_be_bytes()
}

fn cbc_encrypt(key: &[u8; 16], mut chain: [u8; 16], buf: &mut [u8]) {
    let cipher = Aes128::new(key.into());
    for block in buf.as_chunks_mut::<16>().0 {
        for (b, c) in block.iter_mut().zip(chain) {
            *b ^= c;
        }
        cipher.encrypt_block(block.into());
        chain = *block;
    }
}

/// A file on the disc, by its path from the root, `/` separated, and the
/// extents it lies in, in order: a file past 4 GiB takes several.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub path: String,
    /// The first sector and the size in bytes of each.
    pub extents: Vec<(u32, u32)>,
}

impl File {
    pub fn size(&self) -> u64 {
        self.extents.iter().map(|&(_, size)| size as u64).sum()
    }

    /// The sector the file starts at, which an IRD keys its hash by.
    pub fn lba(&self) -> u32 {
        self.extents.first().map_or(0, |&(lba, _)| lba)
    }
}

/// The files of the disc whose plain sectors `read` gives, sector by number:
/// by their Joliet names, which keep the case the primary volume's folds,
/// unless the disc has none. In the order they lie on the disc.
pub fn files(read: &mut dyn FnMut(u32, &mut [u8; SECTOR]) -> Result<()>) -> Result<Vec<File>> {
    let mut sector = [0; SECTOR];
    let (mut primary, mut joliet) = (None, None);
    for lba in 16..48 {
        read(lba, &mut sector)?;
        if &sector[1..6] != b"CD001" {
            return Err(invalid(format!(
                "no volume descriptor in sector {lba}: {}",
                String::from_utf8_lossy(&sector[1..6])
            )));
        }
        match sector[0] {
            1 => primary = Some(root(&sector)),
            // the escape sequences of UCS-2 levels 1 to 3
            2 if matches!(&sector[88..91], b"%/@" | b"%/C" | b"%/E") => {
                joliet = Some(root(&sector))
            }
            255 => break,
            _ => {}
        }
    }
    let (is_joliet, (lba, size)) = match (joliet, primary) {
        (Some(root), _) => (true, root),
        (None, Some(root)) => (false, root),
        (None, None) => return Err(invalid("no volume descriptor".into())),
    };

    let mut files: Vec<File> = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = vec![(String::new(), lba, size)];
    while let Some((path, lba, size)) = stack.pop() {
        // a directory that loops is read once
        if !seen.insert(lba) {
            continue;
        }
        // the extent of a file whose last record said more follow
        let mut continued: Option<usize> = None;
        for at in 0..size.div_ceil(SECTOR as u32) {
            read(lba + at, &mut sector)?;
            let mut offset = 0;
            // records never cross a sector: a zero length pads to the next
            while offset < SECTOR && sector[offset] != 0 {
                let record = &sector[offset..];
                let len = record[0] as usize;
                if len < 34 || offset + len > SECTOR || 33 + record[32] as usize > len {
                    return Err(invalid(format!(
                        "a directory record in sector {}",
                        lba + at
                    )));
                }
                offset += len;
                let extent = u32::from_le_bytes(record[2..6].try_into().unwrap());
                let bytes = u32::from_le_bytes(record[10..14].try_into().unwrap());
                let flags = record[25];
                let raw = &record[33..33 + record[32] as usize];
                if raw == [0] || raw == [1] {
                    continue;
                }
                let name = if is_joliet {
                    let units: Vec<u16> = raw
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| u16::from_be_bytes(*c))
                        .collect();
                    String::from_utf16_lossy(&units)
                } else {
                    String::from_utf8_lossy(raw).into_owned()
                };
                // without its version, and the dot ISO 9660 gives a name with no
                // extension: a file's last, a directory's every one
                let name = name.rsplit_once(';').map_or(&name[..], |(name, _)| name);
                let name = if flags & 0x02 != 0 {
                    name.trim_end_matches('.')
                } else {
                    name.strip_suffix('.').unwrap_or(name)
                };
                let path = if path.is_empty() {
                    name.to_string()
                } else {
                    format!("{path}/{name}")
                };
                if flags & 0x02 != 0 {
                    stack.push((path, extent, bytes));
                    continued = None;
                    continue;
                }
                match continued {
                    Some(i) if files[i].path == path => files[i].extents.push((extent, bytes)),
                    _ => files.push(File {
                        path,
                        extents: vec![(extent, bytes)],
                    }),
                }
                continued = (flags & 0x80 != 0).then_some(files.len() - 1);
            }
        }
    }
    files.sort_by_key(|file| (file.lba(), file.path.clone()));
    Ok(files)
}

/// The root directory's extent and size, from a volume descriptor.
fn root(descriptor: &[u8]) -> (u32, u32) {
    let record = &descriptor[156..190];
    (
        u32::from_le_bytes(record[2..6].try_into().unwrap()),
        u32::from_le_bytes(record[10..14].try_into().unwrap()),
    )
}

/// The disc's size in sectors, from its primary volume descriptor.
pub fn volume_sectors(descriptor: &[u8]) -> u32 {
    u32::from_le_bytes(descriptor[80..84].try_into().unwrap())
}

/// What a file's first bytes are, by its name, when it lies encrypted: an
/// executable's SELF header, a PNG's signature, a PARAM.SFO's.
fn magic(path: &str) -> Option<&'static [u8]> {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_uppercase();
    if name == "EBOOT.BIN" || name.ends_with(".SELF") || name.ends_with(".SPRX") {
        Some(b"SCE\0")
    } else if name.ends_with(".PNG") {
        Some(b"\x89PNG")
    } else if name == "PARAM.SFO" {
        Some(b"\0PSF")
    } else {
        None
    }
}

/// Whether `key` is the disc key of the disc whose sectors `read` gives as
/// they are: what the first encrypted sector of a file whose first bytes are
/// known decrypts to, EBOOT.BIN's when it lies encrypted. `None` when no
/// such file is encrypted, so no key can be told right.
pub fn opens(
    read: &mut dyn FnMut(u32, &mut [u8; SECTOR]) -> Result<()>,
    key: &[u8; 16],
) -> Result<Option<bool>> {
    let mut sector = [0; SECTOR];
    read(0, &mut sector)?;
    let crypt = Crypt::new(key, regions(&sector)?);
    let mut files = files(read)?;
    // EBOOT.BIN first, every disc's executable
    files.sort_by_key(|file| !file.path.eq_ignore_ascii_case("PS3_GAME/USRDIR/EBOOT.BIN"));
    for file in files {
        let (Some(magic), lba) = (magic(&file.path), file.lba()) else {
            continue;
        };
        if file.size() < magic.len() as u64 || !crypt.encrypted(lba) {
            continue;
        }
        read(lba, &mut sector)?;
        crypt.decrypt(lba, &mut sector);
        return Ok(Some(sector.starts_with(magic)));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_alternate_from_the_plain_ones_listed() {
        let mut sector0 = [0; SECTOR];
        sector0[3] = 2;
        sector0[12..16].copy_from_slice(&1311u32.to_be_bytes());
        sector0[16..20].copy_from_slice(&974112u32.to_be_bytes());
        sector0[20..24].copy_from_slice(&1105215u32.to_be_bytes());
        let regions = regions(&sector0).unwrap();
        assert_eq!(
            regions,
            [
                Region {
                    start: 0,
                    end: 1311,
                    encrypted: false
                },
                Region {
                    start: 1312,
                    end: 974111,
                    encrypted: true
                },
                Region {
                    start: 974112,
                    end: 1105215,
                    encrypted: false
                },
            ]
        );
    }

    #[test]
    fn regions_out_of_order_are_refused() {
        let mut sector0 = [0; SECTOR];
        sector0[3] = 2;
        sector0[12..16].copy_from_slice(&100u32.to_be_bytes());
        sector0[16..20].copy_from_slice(&50u32.to_be_bytes());
        sector0[20..24].copy_from_slice(&60u32.to_be_bytes());
        assert!(regions(&sector0).is_err());
    }

    #[test]
    fn sectors_decrypt_to_what_they_encrypted_from() {
        let crypt = Crypt::new(
            &[7; 16],
            vec![
                Region {
                    start: 0,
                    end: 0,
                    encrypted: false,
                },
                Region {
                    start: 1,
                    end: 2,
                    encrypted: true,
                },
            ],
        );
        let plain: Vec<u8> = (0..SECTOR * 3).map(|i| (i * 7 % 251) as u8).collect();
        let mut buf = plain.clone();
        crypt.encrypt(0, &mut buf);
        assert_eq!(buf[..SECTOR], plain[..SECTOR]);
        assert_ne!(buf[SECTOR..], plain[SECTOR..]);
        // each sector chains from its own number, not from the one before
        assert_ne!(buf[SECTOR..SECTOR + 16], buf[2 * SECTOR..2 * SECTOR + 16]);
        crypt.decrypt(0, &mut buf);
        assert_eq!(buf, plain);
    }
}

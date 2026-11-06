//! A GameCube or Wii disc, read through whatever holds it: a raw ISO, or an
//! RVZ. A port of what RVZ needs of Dolphin's `VolumeGC` and `VolumeWii`.

use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use crate::crypto::Key;
use crate::error::{Error, Result};
use crate::fst::{self, FileSystem};
use crate::io::read_exact_at;
use crate::wii::{self, BLOCK_DATA_SIZE, BLOCK_HEADER_SIZE, BLOCK_TOTAL_SIZE};

pub const GAMECUBE_MAGIC: u32 = 0xC2339F3D;
pub const WII_MAGIC: u32 = 0x5D1C9EA3;

/// A partition's header fields, from its start.
pub const PARTITION_TMD_SIZE: u64 = 0x2A4;
pub const PARTITION_TMD_OFFSET: u64 = 0x2A8;
pub const PARTITION_CERT_CHAIN_SIZE: u64 = 0x2AC;
pub const PARTITION_CERT_CHAIN_OFFSET: u64 = 0x2B0;
pub const PARTITION_H3_OFFSET: u64 = 0x2B4;
pub const PARTITION_DATA_OFFSET: u64 = 0x2B8;
pub const PARTITION_DATA_SIZE: u64 = 0x2BC;
pub const PARTITION_H3_SIZE: u64 = 0x18000;

/// What a disc image is read from.
pub trait Source {
    /// The size of the disc.
    fn size(&self) -> u64;
    /// Read the disc at `offset`, failing past its end.
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()>;
    /// The title key of the partition whose data starts at `data_offset`,
    /// if this source keeps it.
    fn title_key(&self, _data_offset: u64) -> Option<Key> {
        None
    }
}

/// A raw disc image.
pub struct Iso {
    file: File,
    size: u64,
}

impl Iso {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let size = file.metadata()?.len();
        Ok(Self { file, size })
    }
}

impl Source for Iso {
    fn size(&self) -> u64 {
        self.size
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        if offset + buf.len() as u64 > self.size {
            return Err(Error::Corrupt(format!(
                "read of {:#x} bytes at {offset:#x} past the end of the disc",
                buf.len()
            )));
        }
        Ok(read_exact_at(&self.file, buf, offset)?)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    GameCube,
    Wii,
}

/// A partition, `None` for the disc outside of any.
pub type Partition = Option<u64>;

/// A disc's structure, read from a [`Source`] passed to each call.
pub struct Volume {
    pub platform: Platform,
    pub has_hashes: bool,
    pub has_encryption: bool,
    /// Partition offsets, in order.
    pub partitions: Vec<u64>,
    common_keys: Vec<Key>,
    keys: HashMap<u64, Key>,
    file_systems: HashMap<Partition, Option<Arc<FileSystem>>>,
}

fn read_u32(source: &mut dyn Source, offset: u64) -> Result<u32> {
    let mut bytes = [0; 4];
    source.read_at(offset, &mut bytes)?;
    Ok(u32::from_be_bytes(bytes))
}

impl Volume {
    /// Read a disc's structure, or `None` if it is neither a GameCube nor a
    /// Wii disc. `common_keys` are tried on each Wii partition's ticket.
    pub fn new(source: &mut dyn Source, common_keys: &[Key]) -> Result<Option<Self>> {
        let platform = if read_u32(source, 0x18).ok() == Some(WII_MAGIC) {
            Platform::Wii
        } else if read_u32(source, 0x1C).ok() == Some(GAMECUBE_MAGIC) {
            Platform::GameCube
        } else {
            return Ok(None);
        };
        let mut volume = Self {
            platform,
            has_hashes: false,
            has_encryption: false,
            partitions: Vec::new(),
            common_keys: common_keys.to_vec(),
            keys: HashMap::new(),
            file_systems: HashMap::new(),
        };
        if platform == Platform::Wii {
            let mut flags = [0; 2];
            source.read_at(0x60, &mut flags)?;
            volume.has_hashes = flags[0] == 0;
            volume.has_encryption = flags[1] == 0;
            let mut partitions = BTreeSet::new();
            for group in 0..4 {
                let table = 0x40000 + group * 8;
                let (Ok(count), Ok(table_offset)) = (
                    read_u32(source, table),
                    volume.read_shifted(source, table + 4, None),
                ) else {
                    continue;
                };
                for i in 0..count as u64 {
                    match volume.read_shifted(source, table_offset + i * 8, None) {
                        Ok(offset) => {
                            partitions.insert(offset);
                        }
                        // Past the end of the disc, as the rest will be
                        Err(_) => break,
                    }
                }
            }
            volume.partitions = partitions.into_iter().collect();
        }
        Ok(Some(volume))
    }

    /// How far a stored offset is shifted: Wii offsets are stored divided by 4.
    pub fn shift(&self) -> u32 {
        match self.platform {
            Platform::Wii => 2,
            Platform::GameCube => 0,
        }
    }

    /// Where a partition's data starts, relative to the partition, 0 if that
    /// cannot be read.
    pub fn data_offset(&self, source: &mut dyn Source, partition: u64) -> u64 {
        self.read_shifted(source, partition + PARTITION_DATA_OFFSET, None)
            .unwrap_or(0)
    }

    pub fn read_u32(
        &mut self,
        source: &mut dyn Source,
        offset: u64,
        partition: Partition,
    ) -> Result<u32> {
        let mut bytes = [0; 4];
        self.read(source, offset, &mut bytes, partition)?;
        Ok(u32::from_be_bytes(bytes))
    }

    /// Read a stored offset or size, shifted back.
    pub fn read_shifted(
        &self,
        source: &mut dyn Source,
        offset: u64,
        partition: Partition,
    ) -> Result<u64> {
        debug_assert!(partition.is_none());
        Ok((read_u32(source, offset)? as u64) << self.shift())
    }

    /// Read a stored offset or size from within a partition, shifted back.
    pub fn read_shifted_in(
        &mut self,
        source: &mut dyn Source,
        offset: u64,
        partition: Partition,
    ) -> Result<u64> {
        Ok((self.read_u32(source, offset, partition)? as u64) << self.shift())
    }

    /// Read a partition's data, decrypted and without its hashes, or the disc
    /// outside of any partition.
    pub fn read(
        &mut self,
        source: &mut dyn Source,
        mut offset: u64,
        mut buf: &mut [u8],
        partition: Partition,
    ) -> Result<()> {
        let Some(partition) = partition else {
            return source.read_at(offset, buf);
        };
        let data_offset = partition + self.data_offset(source, partition);
        if !self.has_hashes {
            return source.read_at(data_offset + offset, buf);
        }
        let key = if self.has_encryption {
            Some(self.key(source, partition)?)
        } else {
            None
        };
        let mut block = vec![0; BLOCK_TOTAL_SIZE as usize];
        let mut data = vec![0; BLOCK_DATA_SIZE as usize];
        while !buf.is_empty() {
            let block_offset = data_offset + offset / BLOCK_DATA_SIZE * BLOCK_TOTAL_SIZE;
            let in_block = (offset % BLOCK_DATA_SIZE) as usize;
            match &key {
                Some(key) => {
                    source.read_at(block_offset, &mut block)?;
                    wii::decrypt_data(key, &block, &mut data);
                }
                None => source.read_at(block_offset + BLOCK_HEADER_SIZE, &mut data)?,
            }
            let length = buf.len().min(BLOCK_DATA_SIZE as usize - in_block);
            buf[..length].copy_from_slice(&data[in_block..in_block + length]);
            buf = &mut buf[length..];
            offset += length as u64;
        }
        Ok(())
    }

    /// A partition's title key: kept by the source, or decrypted from its
    /// ticket with whichever common key decrypts its data.
    pub fn key(&mut self, source: &mut dyn Source, partition: u64) -> Result<Key> {
        if let Some(key) = self.keys.get(&partition) {
            return Ok(*key);
        }
        let data_offset = partition + self.data_offset(source, partition);
        let key = match source.title_key(data_offset) {
            Some(key) => key,
            None => {
                let mut ticket = vec![0; wii::TICKET_SIZE];
                source.read_at(partition, &mut ticket)?;
                let mut block = vec![0; BLOCK_TOTAL_SIZE as usize];
                source.read_at(data_offset, &mut block)?;
                self.common_keys
                    .iter()
                    .map(|common_key| wii::title_key(&ticket, common_key))
                    .find(|key| wii::key_fits(key, &block))
                    .ok_or_else(|| {
                        Error::MissingKey(format!(
                            "no Wii common key given decrypts the partition at {partition:#x}{}",
                            if self.common_keys.is_empty() {
                                ", and none was given"
                            } else {
                                ""
                            }
                        ))
                    })?
            }
        };
        self.keys.insert(partition, key);
        Ok(key)
    }

    /// Where an offset into a partition's data lies on the disc.
    pub fn to_raw_offset(&self, source: &mut dyn Source, offset: u64, partition: Partition) -> u64 {
        let Some(partition) = partition else {
            return offset;
        };
        let data_offset = self.data_offset(source, partition);
        if !self.has_hashes {
            return partition + data_offset + offset;
        }
        partition
            + data_offset
            + offset / BLOCK_DATA_SIZE * BLOCK_TOTAL_SIZE
            + offset % BLOCK_DATA_SIZE
    }

    /// The file system table's offset and size.
    pub fn fst(&mut self, source: &mut dyn Source, partition: Partition) -> Result<(u64, u64)> {
        Ok((
            self.read_shifted_in(source, 0x424, partition)?,
            self.read_shifted_in(source, 0x428, partition)?,
        ))
    }

    /// The boot DOL's offset, `None` where a Datel disc leaves it at 0.
    pub fn dol_offset(
        &mut self,
        source: &mut dyn Source,
        partition: Partition,
    ) -> Result<Option<u64>> {
        let offset = self.read_shifted_in(source, 0x420, partition)?;
        Ok((offset != 0).then_some(offset))
    }

    /// The boot DOL's size: where its furthest section ends.
    pub fn dol_size(
        &mut self,
        source: &mut dyn Source,
        partition: Partition,
        dol: u64,
    ) -> Result<u32> {
        let mut size: u32 = 0;
        for (sections, offsets, sizes) in [(7, 0x00, 0x90), (11, 0x1C, 0xAC)] {
            for i in 0..sections {
                let offset = self.read_u32(source, dol + offsets + i * 4, partition)?;
                let length = self.read_u32(source, dol + sizes + i * 4, partition)?;
                size = size.max(offset.wrapping_add(length));
            }
        }
        Ok(size)
    }

    /// A partition's file system, `None` if it has no valid one, and an
    /// error only if its key is missing.
    pub fn file_system(
        &mut self,
        source: &mut dyn Source,
        partition: Partition,
    ) -> Result<Option<Arc<FileSystem>>> {
        if !self.file_systems.contains_key(&partition) {
            let file_system = self.load_file_system(source, partition)?.map(Arc::new);
            self.file_systems.insert(partition, file_system);
        }
        Ok(self.file_systems[&partition].clone())
    }

    fn load_file_system(
        &mut self,
        source: &mut dyn Source,
        partition: Partition,
    ) -> Result<Option<FileSystem>> {
        // Outside of any partition, only a GameCube disc has one
        if self.platform == Platform::Wii && partition.is_none() {
            return Ok(None);
        }
        let attempt = (|| -> Result<Option<FileSystem>> {
            let shift = if self.read_u32(source, 0x18, partition)? == WII_MAGIC {
                2
            } else if self.read_u32(source, 0x1C, partition)? == GAMECUBE_MAGIC {
                0
            } else {
                return Ok(None);
            };
            let (offset, size) = self.fst(source, partition)?;
            if !(12..=fst::MAX_SIZE).contains(&size) {
                return Ok(None);
            }
            let mut table = vec![0; size as usize];
            self.read(source, offset, &mut table, partition)?;
            Ok(FileSystem::new(table, shift))
        })();
        match attempt {
            Err(error @ Error::MissingKey(_)) => Err(error),
            Err(_) => Ok(None),
            Ok(file_system) => Ok(file_system),
        }
    }
}

//! Reading an RVZ back as the disc it holds: a port of Dolphin's
//! `WIARVZFileReader`.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::ops::Bound;
use std::path::Path;
use std::rc::Rc;

use sha1::{Digest, Sha1};

use crate::compression::{self, Compression};
use crate::crypto::Key;
use crate::disc::Source;
use crate::error::{Error, Result};
use crate::format::*;
use crate::io::read_exact_at;
use crate::lfg::{Lfg, SEED_BYTES};
use crate::wii::{
    self, BLOCK_DATA_SIZE, BLOCK_HEADER_SIZE, BLOCK_TOTAL_SIZE, BLOCKS_PER_GROUP, DIGEST_SIZE,
    GROUP_DATA_SIZE, GROUP_TOTAL_SIZE,
};

#[derive(Clone, Copy, Debug)]
enum DataEntry {
    Raw(usize),
    Partition(usize),
}

/// A group, decompressed and unpacked.
struct Group {
    exceptions: Vec<Vec<Exception>>,
    data: Vec<u8>,
}

/// An RVZ, read as the disc it holds through [`Read`] and [`Seek`], or at
/// any offset with [`Reader::read_at`].
pub struct Reader {
    file: File,
    header_2: Header2,
    iso_size: u64,
    compression: Compression,
    chunk_size: u64,
    partitions: Vec<PartitionEntry>,
    raw_data: Vec<RawDataEntry>,
    groups: Vec<GroupEntry>,
    /// Each entry by where its data ends.
    data_entries: BTreeMap<u64, DataEntry>,
    cached_group: Option<(u32, Rc<Group>)>,
    /// The last Wii group encrypted, by its offset on the disc.
    cached_wii_group: Option<(u64, Vec<u8>)>,
    position: u64,
}

fn sha1(data: &[u8]) -> [u8; 20] {
    Sha1::digest(data).into()
}

impl Reader {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let file_size = file.metadata()?.len();
        let corrupt = |what: &str| Error::Corrupt(format!("{}: {what}", path.display()));

        let mut bytes = [0; HEADER_1_SIZE];
        read_exact_at(&file, &mut bytes, 0)?;
        let header_1 = Header1::parse(&bytes);
        if header_1.magic == *b"WIA\x01" {
            return Err(Error::Unsupported("WIA, RVZ's predecessor".into()));
        }
        if header_1.magic != MAGIC {
            return Err(Error::BadMagic {
                expected: "RVZ\\x01".into(),
                found: header_1.magic.escape_ascii().to_string(),
            });
        }
        if VERSION < header_1.version_compatible || VERSION_READ_COMPATIBLE > header_1.version {
            return Err(Error::Unsupported(format!(
                "RVZ version {:#010x}",
                header_1.version
            )));
        }
        if sha1(&bytes[..HEADER_1_SIZE - DIGEST_SIZE]) != header_1.header_1_hash {
            return Err(corrupt("first header hash mismatch"));
        }
        if header_1.file_size != file_size {
            return Err(corrupt("file size mismatch"));
        }

        let header_2_size = header_1.header_2_size as usize;
        let header_2_min_size = HEADER_2_SIZE - 7;
        if header_2_size < header_2_min_size {
            return Err(corrupt("second header too small"));
        }
        let mut bytes = vec![0; header_2_size];
        read_exact_at(&file, &mut bytes, HEADER_1_SIZE as u64)?;
        if sha1(&bytes) != header_1.header_2_hash {
            return Err(corrupt("second header hash mismatch"));
        }
        let header_2 = Header2::parse(&bytes);
        if header_2.compressor_data_size > 7
            || header_2_size < header_2_min_size + header_2.compressor_data_size as usize
        {
            return Err(corrupt("bad compressor data"));
        }
        let chunk_size = header_2.chunk_size as u64;
        if (chunk_size < BLOCK_TOTAL_SIZE || !chunk_size.is_power_of_two())
            && !chunk_size.is_multiple_of(GROUP_TOTAL_SIZE)
        {
            return Err(corrupt("bad chunk size"));
        }
        let compression = Compression::from_id(header_2.compression_type).ok_or_else(|| {
            Error::Unsupported(format!("compression method {}", header_2.compression_type))
        })?;

        let mut reader = Self {
            file,
            iso_size: header_1.iso_file_size,
            compression,
            chunk_size,
            partitions: Vec::new(),
            raw_data: Vec::new(),
            groups: Vec::new(),
            data_entries: BTreeMap::new(),
            cached_group: None,
            cached_wii_group: None,
            position: 0,
            header_2,
        };

        let entry_size = reader.header_2.partition_entry_size as usize;
        let mut bytes = vec![0; entry_size * reader.header_2.partition_entries as usize];
        read_exact_at(
            &reader.file,
            &mut bytes,
            reader.header_2.partition_entries_offset,
        )?;
        if sha1(&bytes) != reader.header_2.partition_entries_hash {
            return Err(corrupt("partition entries hash mismatch"));
        }
        for entry in bytes
            .chunks(entry_size.max(1))
            .take(reader.header_2.partition_entries as usize)
        {
            let mut full = [0; PARTITION_ENTRY_SIZE];
            let length = entry.len().min(PARTITION_ENTRY_SIZE);
            full[..length].copy_from_slice(&entry[..length]);
            reader.partitions.push(PartitionEntry::parse(&full));
        }
        for (i, partition) in reader.partitions.iter().enumerate() {
            let mut non_empty = 0;
            for data in &partition.data {
                if data.sectors != 0 {
                    non_empty += 1;
                    let end = (data.first_sector as u64 + data.sectors as u64) * BLOCK_TOTAL_SIZE;
                    reader
                        .data_entries
                        .entry(end)
                        .or_insert(DataEntry::Partition(i));
                }
            }
            if non_empty > 1 && partition.data[0].first_sector > partition.data[1].first_sector {
                return Err(corrupt("partition data out of order"));
            }
        }

        let raw_data = reader.read_table(
            reader.header_2.raw_data_entries_offset,
            reader.header_2.raw_data_entries_size,
            reader.header_2.raw_data_entries as usize * RAW_DATA_ENTRY_SIZE,
        )?;
        reader.raw_data = raw_data
            .chunks(RAW_DATA_ENTRY_SIZE)
            .map(RawDataEntry::parse)
            .collect();
        for (i, raw) in reader.raw_data.iter().enumerate() {
            if raw.data_size != 0 {
                reader
                    .data_entries
                    .entry(raw.data_offset + raw.data_size)
                    .or_insert(DataEntry::Raw(i));
            }
        }

        let groups = reader.read_table(
            reader.header_2.group_entries_offset,
            reader.header_2.group_entries_size,
            reader.header_2.group_entries as usize * GROUP_ENTRY_SIZE,
        )?;
        reader.groups = groups
            .chunks(GROUP_ENTRY_SIZE)
            .map(GroupEntry::parse)
            .collect();

        if reader.has_data_overlap() {
            return Err(corrupt("overlapping data"));
        }
        Ok(reader)
    }

    /// Read one of the compressed tables the second header points to.
    fn read_table(&self, offset: u64, size: u32, expected: usize) -> Result<Vec<u8>> {
        let mut bytes = vec![0; size as usize];
        read_exact_at(&self.file, &mut bytes, offset)?;
        let table = compression::decompress(self.compression, self.compressor_data(), &bytes)?;
        if table.len() != expected {
            return Err(Error::Corrupt("table size mismatch".into()));
        }
        Ok(table)
    }

    fn compressor_data(&self) -> &[u8] {
        &self.header_2.compressor_data[..self.header_2.compressor_data_size as usize]
    }

    fn entry_after(&self, offset: u64) -> Option<DataEntry> {
        self.data_entries
            .range((Bound::Excluded(offset), Bound::Unbounded))
            .next()
            .map(|(_, &entry)| entry)
    }

    fn has_data_overlap(&self) -> bool {
        for (i, partition) in self.partitions.iter().enumerate() {
            for data in partition.data.iter().filter(|data| data.sectors != 0) {
                match self.entry_after(data.first_sector as u64 * BLOCK_TOTAL_SIZE) {
                    Some(DataEntry::Partition(index)) if index == i => {}
                    _ => return true,
                }
            }
        }
        for (i, raw) in self.raw_data.iter().enumerate() {
            if raw.data_size == 0 {
                continue;
            }
            match self.entry_after(raw.data_offset) {
                Some(DataEntry::Raw(index)) if index == i => {}
                _ => return true,
            }
        }
        false
    }

    /// The size of the disc.
    pub fn size(&self) -> u64 {
        self.iso_size
    }

    pub fn compression(&self) -> Compression {
        self.compression
    }

    pub fn compression_level(&self) -> i32 {
        self.header_2.compression_level
    }

    pub fn chunk_size(&self) -> u32 {
        self.header_2.chunk_size
    }

    /// Read the disc at `offset`.
    pub fn read_at(&mut self, mut offset: u64, mut buf: &mut [u8]) -> Result<()> {
        if offset + buf.len() as u64 > self.iso_size {
            return Err(Error::Corrupt(format!(
                "read of {:#x} bytes at {offset:#x} past the end of the disc",
                buf.len()
            )));
        }
        if offset < DISC_HEADER_SIZE as u64 {
            let length = buf.len().min(DISC_HEADER_SIZE - offset as usize);
            buf[..length].copy_from_slice(&self.header_2.disc_header[offset as usize..][..length]);
            offset += length as u64;
            buf = &mut buf[length..];
        }
        while !buf.is_empty() {
            let entry = self
                .entry_after(offset)
                .ok_or_else(|| Error::Corrupt(format!("no data for offset {offset:#x}")))?;
            let before = buf.len();
            match entry {
                DataEntry::Raw(index) => {
                    let raw = self.raw_data[index];
                    let chunk_size = self.chunk_size;
                    self.read_groups(
                        &mut offset,
                        &mut buf,
                        chunk_size,
                        BLOCK_TOTAL_SIZE,
                        raw.data_offset,
                        raw.data_size,
                        raw.group_index,
                        raw.groups,
                        0,
                        None,
                    )?;
                }
                DataEntry::Partition(index) => self.read_partition(index, &mut offset, &mut buf)?,
            }
            if buf.len() == before {
                return Err(Error::Corrupt(format!("no data for offset {offset:#x}")));
            }
        }
        Ok(())
    }

    /// Read a partition's part of the disc, re-encrypted, its hashes
    /// recomputed and any exceptions to them applied.
    fn read_partition(
        &mut self,
        index: usize,
        offset: &mut u64,
        buf: &mut &mut [u8],
    ) -> Result<()> {
        let partition = self.partitions[index];
        let first_sector = partition.data[0].first_sector as u64;
        let data_offset = first_sector * BLOCK_TOTAL_SIZE;
        let total_sectors = if partition.data[1].sectors != 0 {
            partition.data[1].first_sector as u64 - first_sector + partition.data[1].sectors as u64
        } else {
            partition.data[0].sectors as u64
        };
        for data in partition.data {
            if buf.is_empty() {
                return Ok(());
            }
            let start = data.first_sector as u64 * BLOCK_TOTAL_SIZE;
            let size = data.sectors as u64 * BLOCK_TOTAL_SIZE;
            if size == 0 || start + size <= *offset {
                continue;
            }
            if *offset < start {
                return Err(Error::Corrupt(format!("no data for offset {offset:#x}")));
            }
            let mut length = (size - (*offset - start)).min(buf.len() as u64) as usize;
            while length > 0 {
                let in_partition = *offset - data_offset;
                let group_offset = in_partition / GROUP_TOTAL_SIZE * GROUP_TOTAL_SIZE;
                let in_group = (in_partition - group_offset) as usize;
                let disc_offset = data_offset + group_offset;
                if self.cached_wii_group.as_ref().map(|(at, _)| *at) != Some(disc_offset) {
                    let group = self.encrypt_group(
                        index,
                        group_offset / GROUP_TOTAL_SIZE * GROUP_DATA_SIZE,
                        total_sectors * BLOCK_DATA_SIZE,
                    )?;
                    self.cached_wii_group = Some((disc_offset, group));
                }
                let group = &self.cached_wii_group.as_ref().unwrap().1;
                let count = length.min(GROUP_TOTAL_SIZE as usize - in_group);
                buf[..count].copy_from_slice(&group[in_group..in_group + count]);
                *buf = &mut std::mem::take(buf)[count..];
                *offset += count as u64;
                length -= count;
            }
        }
        Ok(())
    }

    /// Rebuild a Wii group as it is on the disc from its decrypted data.
    fn encrypt_group(&mut self, index: usize, offset: u64, decrypted_size: u64) -> Result<Vec<u8>> {
        let mut data = vec![[0; BLOCK_DATA_SIZE as usize]; BLOCKS_PER_GROUP as usize];
        let mut exceptions = Vec::new();
        let mut last_group = None;
        for (block, block_data) in data.iter_mut().enumerate() {
            let block_offset = offset + block as u64 * BLOCK_DATA_SIZE;
            if block_offset + BLOCK_DATA_SIZE <= decrypted_size {
                self.read_decrypted(
                    index,
                    block_offset,
                    block_data,
                    Some((&mut exceptions, &mut last_group)),
                )?;
            }
        }
        let mut hashes = wii::hash_group(&data);
        for exception in &exceptions {
            let block = exception.offset as usize / BLOCK_HEADER_SIZE as usize;
            let in_block = exception.offset as usize % BLOCK_HEADER_SIZE as usize;
            if block >= BLOCKS_PER_GROUP as usize
                || in_block + DIGEST_SIZE > BLOCK_HEADER_SIZE as usize
            {
                return Err(Error::Corrupt("hash exception out of range".into()));
            }
            hashes[block][in_block..in_block + DIGEST_SIZE].copy_from_slice(&exception.hash);
        }
        let mut out = vec![0; GROUP_TOTAL_SIZE as usize];
        let key = self.partitions[index].key;
        wii::encrypt_group(&key, &hashes, &data, &mut out);
        Ok(out)
    }

    /// Read a partition's decrypted data, collecting the hash exceptions of
    /// each group read into `exceptions` if given.
    fn read_decrypted(
        &mut self,
        index: usize,
        mut offset: u64,
        mut buf: &mut [u8],
        mut exceptions: Option<(&mut Vec<Exception>, &mut Option<u32>)>,
    ) -> Result<()> {
        let partition = self.partitions[index];
        let first_sector = partition.data[0].first_sector as u64;
        let chunk_size = self.chunk_size * BLOCK_DATA_SIZE / BLOCK_TOTAL_SIZE;
        let lists = (chunk_size / GROUP_DATA_SIZE).max(1) as u32;
        for data in partition.data {
            if buf.is_empty() {
                break;
            }
            self.read_groups(
                &mut offset,
                &mut buf,
                chunk_size,
                BLOCK_DATA_SIZE,
                (data.first_sector as u64 - first_sector) * BLOCK_DATA_SIZE,
                data.sectors as u64 * BLOCK_DATA_SIZE,
                data.group_index,
                data.groups,
                lists,
                exceptions
                    .as_mut()
                    .map(|(list, last)| (&mut **list, &mut **last)),
            )?;
        }
        if !buf.is_empty() {
            return Err(Error::Corrupt(format!("no partition data at {offset:#x}")));
        }
        Ok(())
    }

    /// Read what a run of groups holds, `data_offset` and `data_size` being
    /// where the run lies, in units of the data it holds.
    #[allow(clippy::too_many_arguments)]
    fn read_groups(
        &mut self,
        offset: &mut u64,
        buf: &mut &mut [u8],
        mut chunk_size: u64,
        sector_size: u64,
        mut data_offset: u64,
        mut data_size: u64,
        group_index: u32,
        groups: u32,
        exception_lists: u32,
        mut exceptions: Option<(&mut Vec<Exception>, &mut Option<u32>)>,
    ) -> Result<()> {
        if data_offset + data_size <= *offset {
            return Ok(());
        }
        if *offset < data_offset {
            return Err(Error::Corrupt(format!("no data for offset {offset:#x}")));
        }
        let skipped = data_offset % sector_size;
        data_offset -= skipped;
        data_size += skipped;

        let start = (*offset - data_offset) / chunk_size;
        for i in start..groups as u64 {
            if buf.is_empty() {
                break;
            }
            let index = group_index as u64 + i;
            if index >= self.groups.len() as u64 {
                return Err(Error::Corrupt("group index out of range".into()));
            }
            let index = index as u32;
            let in_data = i * chunk_size;
            let in_group = *offset - in_data - data_offset;
            chunk_size = chunk_size.min(data_size - in_data);
            let length = (chunk_size - in_group).min(buf.len() as u64) as usize;

            let group = self.group(index, chunk_size as usize, exception_lists, in_data)?;
            buf[..length]
                .copy_from_slice(&group.data[in_group as usize..in_group as usize + length]);

            if let Some((list, last)) = exceptions.as_mut()
                && **last != Some(index)
            {
                let list_index = (in_group / GROUP_DATA_SIZE) as usize;
                let additional =
                    (in_data % GROUP_DATA_SIZE / BLOCK_DATA_SIZE * BLOCK_HEADER_SIZE) as u16;
                let found = group
                    .exceptions
                    .get(list_index)
                    .ok_or_else(|| Error::Corrupt("missing hash exceptions".into()))?;
                list.extend(found.iter().map(|exception| Exception {
                    offset: exception.offset.wrapping_add(additional),
                    hash: exception.hash,
                }));
                **last = Some(index);
            }

            *buf = &mut std::mem::take(buf)[length..];
            *offset += length as u64;
        }
        Ok(())
    }

    /// A group, decompressed and unpacked: `size` bytes of data after
    /// `exception_lists` lists of hash exceptions.
    fn group(
        &mut self,
        index: u32,
        size: usize,
        exception_lists: u32,
        data_offset: u64,
    ) -> Result<Rc<Group>> {
        if let Some((cached, group)) = &self.cached_group
            && *cached == index
        {
            return Ok(group.clone());
        }
        let entry = self.groups[index as usize];
        let compressed = entry.data_size & 0x8000_0000 != 0;
        let stored = (entry.data_size & 0x7FFF_FFFF) as usize;
        let group = if stored == 0 {
            Group {
                exceptions: vec![Vec::new(); exception_lists as usize],
                data: vec![0; size],
            }
        } else {
            let mut bytes = vec![0; stored];
            read_exact_at(&self.file, &mut bytes, (entry.data_offset as u64) << 2)?;
            let (exceptions, rest) = if compressed {
                let mut out =
                    compression::decompress(self.compression, self.compressor_data(), &bytes)?;
                let (exceptions, used) = parse_exception_lists(&out, exception_lists)?;
                out.drain(..used);
                (exceptions, out)
            } else {
                let (exceptions, mut used) = parse_exception_lists(&bytes, exception_lists)?;
                if exception_lists > 0 {
                    used = used.next_multiple_of(4);
                }
                if used > bytes.len() {
                    return Err(Error::Corrupt("truncated hash exceptions".into()));
                }
                bytes.drain(..used);
                (exceptions, bytes)
            };
            let data = if entry.packed_size != 0 {
                if rest.len() != entry.packed_size as usize {
                    return Err(Error::Corrupt("packed size mismatch".into()));
                }
                unpack(&rest, size, data_offset)?
            } else {
                if rest.len() != size {
                    return Err(Error::Corrupt("group size mismatch".into()));
                }
                rest
            };
            Group { exceptions, data }
        };
        let group = Rc::new(group);
        self.cached_group = Some((index, group.clone()));
        Ok(group)
    }

    /// Each partition's title key, by where its data starts on the disc.
    pub fn title_keys(&self) -> impl Iterator<Item = (u64, Key)> + '_ {
        self.partitions.iter().map(|partition| {
            (
                partition.data[0].first_sector as u64 * BLOCK_TOTAL_SIZE,
                partition.key,
            )
        })
    }
}

/// Undo RVZ packing: runs of data, and runs of junk stored as the seed that
/// generates them, into `size` bytes. `data_offset` is where the data lies,
/// which the junk depends on.
fn unpack(packed: &[u8], size: usize, mut data_offset: u64) -> Result<Vec<u8>> {
    let corrupt = || Error::Corrupt("bad RVZ packing".into());
    let mut out = Vec::with_capacity(size);
    let mut position = 0;
    while out.len() < size {
        let header = packed.get(position..position + 4).ok_or_else(corrupt)?;
        let header = u32::from_be_bytes(header.try_into().unwrap());
        position += 4;
        let length = (header & 0x7FFF_FFFF) as usize;
        if out.len() + length > size {
            return Err(corrupt());
        }
        if header & 0x8000_0000 != 0 {
            let seed = packed
                .get(position..position + SEED_BYTES)
                .ok_or_else(corrupt)?;
            position += SEED_BYTES;
            let mut lfg = Lfg::from_bytes(seed);
            lfg.forward_bytes((data_offset % BLOCK_TOTAL_SIZE) as usize);
            let start = out.len();
            out.resize(start + length, 0);
            lfg.fill(&mut out[start..]);
        } else {
            out.extend_from_slice(
                packed
                    .get(position..position + length)
                    .ok_or_else(corrupt)?,
            );
            position += length;
        }
        data_offset += length as u64;
    }
    if position != packed.len() {
        return Err(corrupt());
    }
    Ok(out)
}

impl Source for Reader {
    fn size(&self) -> u64 {
        self.iso_size
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        Reader::read_at(self, offset, buf)
    }

    fn title_key(&self, data_offset: u64) -> Option<Key> {
        self.title_keys()
            .find(|(offset, _)| *offset == data_offset)
            .map(|(_, key)| key)
    }
}

impl Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let length = (buf.len() as u64).min(self.iso_size.saturating_sub(self.position)) as usize;
        self.read_at(self.position, &mut buf[..length])
            .map_err(|error| match error {
                Error::Io(error) => error,
                error => io::Error::new(io::ErrorKind::InvalidData, error.to_string()),
            })?;
        self.position += length as u64;
        Ok(length)
    }
}

impl Seek for Reader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let position = match pos {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::End(delta) => self.iso_size.checked_add_signed(delta),
            SeekFrom::Current(delta) => self.position.checked_add_signed(delta),
        }
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before the start"))?;
        self.position = position;
        Ok(position)
    }
}

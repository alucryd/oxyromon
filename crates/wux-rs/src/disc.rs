//! The layout of a Wii U disc, as Cemu reads it: a partition table encrypted
//! with the disc key, then partitions, each with a file system table (FST)
//! whose files are split across clusters, each cluster either one raw AES-CBC
//! stream or a run of hashed 64 KiB blocks.

use std::rc::Rc;

use sha1::{Digest, Sha1};

use crate::crypto::{Key, cbc_decrypt, decrypt_block};
use crate::error::{Error, Result, corrupt};
use crate::image::Image;

/// The disc's sector size, which every offset below counts in.
pub const SECTOR: u64 = 0x8000;
/// A hashed block: 0x400 bytes of hashes, then 0xFC00 of data.
const HASHED_BLOCK: u64 = 0x10000;
const HASHES: usize = 0x400;
const HASHED_DATA: u64 = HASHED_BLOCK - HASHES as u64;

const DISC_MAGIC: u32 = 0xCC54_9EB9;
const TABLE_MAGIC: u32 = 0xCCA6_E67B;
const PARTITION_MAGIC: u32 = 0xCC93_A4F5;
const FST_MAGIC: u32 = 0x4653_5400;

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes(bytes[at..at + 2].try_into().unwrap())
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// Fail unless the image is a Wii U disc.
pub fn check_magic(image: &Image) -> Result<()> {
    let mut magic = [0; 4];
    image.read_exact_at(&mut magic, 2 * SECTOR)?;
    if u32::from_be_bytes(magic) != DISC_MAGIC {
        return Err(Error::BadMagic {
            expected: format!("{DISC_MAGIC:#010x}"),
            found: format!("{:#010x}", u32::from_be_bytes(magic)),
        });
    }
    Ok(())
}

/// The key among `keys` that decrypts the partition table: the one that turns
/// the zero padding past its header back into zeros, as Cemu checks.
pub fn find_disc_key(image: &Image, keys: &[Key]) -> Result<Key> {
    let mut bytes = [0; 48];
    image.read_exact_at(&mut bytes, 3 * SECTOR + 0x100)?;
    let iv: [u8; 16] = bytes[..16].try_into().unwrap();
    keys.iter()
        .find(|key| {
            let mut padding = bytes[16..].to_vec();
            cbc_decrypt(key, &mut iv.clone(), &mut padding);
            padding.iter().all(|&b| b == 0)
        })
        .copied()
        .ok_or_else(|| Error::MissingKey("none of the keys is this disc's key".into()))
}

/// A partition: its name, and where it starts.
pub struct Partition {
    pub name: String,
    pub offset: u64,
}

/// The partitions, from the table the disc key decrypts.
pub fn partitions(image: &Image, disc_key: &Key) -> Result<Vec<Partition>> {
    let mut table = vec![0; SECTOR as usize];
    image.read_exact_at(&mut table, 3 * SECTOR)?;
    cbc_decrypt(disc_key, &mut [0; 16], &mut table);
    if u32_at(&table, 0) != TABLE_MAGIC {
        return Err(corrupt("the partition table does not decrypt"));
    }
    if u64::from(u32_at(&table, 4)) != SECTOR {
        return Err(corrupt("the partition table's block size is not 0x8000"));
    }
    let count = u32_at(&table, 0x1C) as usize;
    // Cemu's limit; the sector has room for 240.
    if count > 30 {
        return Err(corrupt(format!("the disc has {count} partitions")));
    }
    table[0x800..]
        .as_chunks::<0x80>()
        .0
        .iter()
        .take(count)
        .map(|entry| {
            let name = &entry[..31];
            let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(31)];
            if entry[31] != 1 {
                return Err(Error::Unsupported(format!(
                    "a partition with {} addresses",
                    entry[31]
                )));
            }
            Ok(Partition {
                name: String::from_utf8_lossy(name).into_owned(),
                offset: u64::from(u32_at(entry, 0x20)) * SECTOR,
            })
        })
        .collect()
}

/// Where a partition's FST is, from the partition's header, which is not
/// encrypted: its offset on the disc and its size.
pub fn fst_location(image: &Image, partition: &Partition) -> Result<(u64, u32)> {
    let mut header = [0; 0x20];
    image.read_exact_at(&mut header, partition.offset)?;
    if u32_at(&header, 0) != PARTITION_MAGIC {
        return Err(corrupt(format!(
            "partition {} has no partition header",
            partition.name
        )));
    }
    let size = u32_at(&header, 0x14);
    let offset = partition.offset + u64::from(u32_at(&header, 0x18)) * SECTOR;
    Ok((offset, size))
}

/// Read an FST's encrypted bytes, padded to the AES block size.
pub fn read_fst(image: &Image, (offset, size): (u64, u32)) -> Result<Vec<u8>> {
    let mut bytes = vec![0; (size as usize).next_multiple_of(16)];
    image.read_exact_at(&mut bytes, offset)?;
    Ok(bytes)
}

/// Whether `key` decrypts the FST starting with `encrypted`.
pub fn opens_fst(encrypted: &[u8], key: &Key) -> bool {
    encrypted.len() >= 16
        && u32_at(
            &decrypt_block(key, &[0; 16], encrypted[..16].try_into().unwrap()),
            0,
        ) == FST_MAGIC
}

/// Where an FST's clusters are read from: a disc partition, or the `.app`
/// files of a title on Nintendo's servers (NUS).
pub trait Source {
    /// Fill `buf` from `offset` bytes into cluster `index`, which the FST
    /// places `start` bytes into its partition.
    fn read_cluster(&self, index: u16, start: u64, offset: u64, buf: &mut [u8]) -> Result<()>;
}

/// The clusters of the disc partition starting at `offset`.
pub struct PartitionSource {
    pub image: Rc<Image>,
    pub offset: u64,
}

impl Source for PartitionSource {
    fn read_cluster(&self, _index: u16, start: u64, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.image.read_exact_at(buf, self.offset + start + offset)
    }
}

struct Cluster {
    /// Where it starts in its partition; NUS titles keep each in a file.
    offset: u64,
    hashed: bool,
}

/// An FST entry.
pub struct Entry {
    flags: u8,
    name: u32,
    /// For a file, its offset in its cluster in `offset_factor`s; for a
    /// directory, its parent's index.
    offset: u32,
    /// For a file, its size; for a directory, the index past its last entry.
    size: u32,
    cluster: u16,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.flags & 1 != 0
    }

    /// Links are hidden from the title's file system; Cemu skips them.
    pub fn is_link(&self) -> bool {
        self.flags & 0x80 != 0
    }

    /// A file's size.
    pub fn size(&self) -> u64 {
        u64::from(self.size)
    }

    /// The index past a directory's last entry.
    pub fn end(&self) -> usize {
        self.size as usize
    }
}

/// A partition's decrypted FST, and the key its files decrypt with.
pub struct Fst {
    key: Key,
    offset_factor: u64,
    clusters: Vec<Cluster>,
    pub entries: Vec<Entry>,
    names: Vec<u8>,
}

impl Fst {
    /// Decrypt and check the FST of `name`, a partition or a title.
    pub fn open(name: &str, mut bytes: Vec<u8>, size: u32, key: Key) -> Result<Fst> {
        cbc_decrypt(&key, &mut [0; 16], &mut bytes);
        bytes.truncate(size as usize);
        if bytes.len() < 0x20 || u32_at(&bytes, 0) != FST_MAGIC {
            return Err(corrupt(format!("the FST of {name} does not decrypt")));
        }
        let offset_factor = u64::from(u32_at(&bytes, 4));
        let cluster_count = u32_at(&bytes, 8) as usize;
        if cluster_count >= 0x1000 {
            return Err(corrupt(format!("the FST has {cluster_count} clusters")));
        }
        let entries_at = 0x20 + cluster_count * 0x20;
        let short = || corrupt("the FST is cut short");
        let clusters = bytes
            .get(0x20..entries_at)
            .ok_or_else(short)?
            .as_chunks::<0x20>()
            .0
            .iter()
            .map(|c| Cluster {
                offset: u64::from(u32_at(c, 0)) * SECTOR,
                hashed: c[0x14] == 2,
            })
            .collect();
        let count = u32_at(
            bytes.get(entries_at..entries_at + 0x10).ok_or_else(short)?,
            8,
        ) as usize;
        let names_at = count
            .checked_mul(0x10)
            .and_then(|n| n.checked_add(entries_at))
            .filter(|&n| n <= bytes.len())
            .ok_or_else(short)?;
        let entries: Vec<Entry> = bytes[entries_at..names_at]
            .as_chunks::<0x10>()
            .0
            .iter()
            .map(|e| Entry {
                flags: e[0],
                name: u32_at(e, 0) & 0xFF_FFFF,
                offset: u32_at(e, 4),
                size: u32_at(e, 8),
                cluster: u16_at(e, 0xE),
            })
            .collect();
        let fst = Fst {
            key,
            offset_factor,
            clusters,
            entries,
            names: bytes[names_at..].to_vec(),
        };
        fst.validate()?;
        Ok(fst)
    }

    /// Check what reading the tree relies on: names in bounds, directories
    /// nested, files in existing clusters.
    fn validate(&self) -> Result<()> {
        let count = self.entries.len();
        let root = self
            .entries
            .first()
            .ok_or_else(|| corrupt("the FST is empty"))?;
        if !root.is_dir() || root.end() != count {
            return Err(corrupt("the FST's root is not a directory spanning it"));
        }
        let mut ends = vec![count];
        for (i, entry) in self.entries.iter().enumerate().skip(1) {
            while i >= *ends.last().unwrap() {
                ends.pop();
            }
            self.name(i)?;
            if entry.is_dir() {
                if entry.end() <= i || entry.end() > *ends.last().unwrap() {
                    return Err(corrupt(format!("FST directory {i} is out of bounds")));
                }
                ends.push(entry.end());
            } else if entry.cluster as usize >= self.clusters.len() {
                return Err(corrupt(format!("FST file {i} is in a missing cluster")));
            }
        }
        Ok(())
    }

    /// The key the FST's files decrypt with.
    pub fn key(&self) -> &Key {
        &self.key
    }

    /// File `index`'s cluster, its offset in the cluster's data, and its size.
    pub fn extent(&self, index: usize) -> (u16, u64, u64) {
        let entry = &self.entries[index];
        (
            entry.cluster,
            u64::from(entry.offset) * self.offset_factor,
            entry.size(),
        )
    }

    /// Where cluster `index` starts in its partition.
    pub fn cluster_offset(&self, index: u16) -> Option<u64> {
        self.clusters
            .get(index as usize)
            .map(|cluster| cluster.offset)
    }

    /// Entry `index`'s name.
    pub fn name(&self, index: usize) -> Result<&[u8]> {
        let start = self.entries[index].name as usize;
        let rest = self.names.get(start..).unwrap_or_default();
        let len = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| corrupt(format!("FST entry {index}'s name is out of bounds")))?;
        Ok(&rest[..len])
    }

    /// The file at `path`, `/`-separated and case-insensitive, as the Wii U
    /// matches names.
    pub fn find(&self, path: &str) -> Option<usize> {
        let (mut index, mut end) = (1, self.entries.len());
        let mut components = path.split('/').peekable();
        while let Some(component) = components.next() {
            let mut found = None;
            while index < end {
                let entry = &self.entries[index];
                if self
                    .name(index)
                    .ok()?
                    .eq_ignore_ascii_case(component.as_bytes())
                {
                    found = Some(index);
                    break;
                }
                index = if entry.is_dir() {
                    entry.end()
                } else {
                    index + 1
                };
            }
            let at = found?;
            let is_dir = self.entries[at].is_dir();
            if components.peek().is_none() {
                return (!is_dir).then_some(at);
            }
            if !is_dir {
                return None;
            }
            (index, end) = (at + 1, self.entries[at].end());
        }
        None
    }

    /// Read file `index` whole.
    pub fn read(&self, source: &dyn Source, index: usize) -> Result<Vec<u8>> {
        let mut data = Vec::with_capacity(self.entries[index].size() as usize);
        self.read_with(source, index, &mut |chunk| {
            data.extend_from_slice(chunk);
            Ok(())
        })?;
        Ok(data)
    }

    /// Decrypt file `index`, handing it to `sink` a piece at a time.
    pub fn read_with(
        &self,
        source: &dyn Source,
        index: usize,
        sink: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<()> {
        let entry = &self.entries[index];
        let cluster = &self.clusters[entry.cluster as usize];
        let start = u64::from(entry.offset) * self.offset_factor;
        if cluster.hashed {
            self.read_hashed(source, entry.cluster, cluster, start, entry.size(), sink)
        } else {
            self.read_raw(source, entry.cluster, cluster, start, entry.size(), sink)
        }
    }

    /// A raw cluster is one CBC stream, whose IV is the cluster's index.
    fn read_raw(
        &self,
        source: &dyn Source,
        index: u16,
        cluster: &Cluster,
        start: u64,
        size: u64,
        sink: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<()> {
        let mut sector = start / SECTOR;
        let mut iv = [0; 16];
        if sector == 0 {
            iv[..2].copy_from_slice(&index.to_be_bytes());
        } else {
            source.read_cluster(index, cluster.offset, sector * SECTOR - 16, &mut iv)?;
        }
        let (mut skip, mut left) = ((start % SECTOR) as usize, size);
        let mut buf = vec![0; 32 * SECTOR as usize];
        while left > 0 {
            let len = (skip as u64 + left)
                .next_multiple_of(SECTOR)
                .min(buf.len() as u64) as usize;
            source.read_cluster(index, cluster.offset, sector * SECTOR, &mut buf[..len])?;
            cbc_decrypt(&self.key, &mut iv, &mut buf[..len]);
            let take = ((len - skip) as u64).min(left) as usize;
            sink(&buf[skip..skip + take])?;
            left -= take as u64;
            sector += len as u64 / SECTOR;
            skip = 0;
        }
        Ok(())
    }

    /// A hashed block's data decrypts with its SHA-1 as IV, which the
    /// block's own hash area holds, along with those of 15 neighbours.
    fn read_hashed(
        &self,
        source: &dyn Source,
        index: u16,
        cluster: &Cluster,
        start: u64,
        size: u64,
        sink: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<()> {
        let mut block_index = start / HASHED_DATA;
        let (mut skip, mut left) = ((start % HASHED_DATA) as usize, size);
        let mut block = vec![0; HASHED_BLOCK as usize];
        while left > 0 {
            source.read_cluster(
                index,
                cluster.offset,
                block_index * HASHED_BLOCK,
                &mut block,
            )?;
            let (hashes, data) = block.split_at_mut(HASHES);
            cbc_decrypt(&self.key, &mut [0; 16], hashes);
            let h0 = &hashes[(block_index % 16) as usize * 20..][..20];
            cbc_decrypt(&self.key, &mut h0[..16].try_into().unwrap(), data);
            if Sha1::digest(&*data).as_slice() != h0 {
                return Err(corrupt(format!(
                    "hashed block {block_index} fails its hash check"
                )));
            }
            let take = ((data.len() - skip) as u64).min(left) as usize;
            sink(&data[skip..skip + take])?;
            left -= take as u64;
            block_index += 1;
            skip = 0;
        }
        Ok(())
    }
}

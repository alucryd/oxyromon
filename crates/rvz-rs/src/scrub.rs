//! Scrubbing: zeroing every 32 KiB cluster of a disc that nothing on it
//! points to, the padding a disc is filled with, so it stores as next to
//! nothing. A scrubbed disc plays the same, but is no longer the disc it was.
//! A port of Dolphin's `DiscScrubber`.

use crate::disc::{
    PARTITION_CERT_CHAIN_OFFSET, PARTITION_CERT_CHAIN_SIZE, PARTITION_DATA_OFFSET,
    PARTITION_H3_OFFSET, PARTITION_H3_SIZE, PARTITION_TMD_OFFSET, PARTITION_TMD_SIZE, Partition,
    Source, Volume,
};
use crate::error::{Error, Result};

pub const CLUSTER_SIZE: u64 = 0x8000;

pub struct Scrubber {
    /// Whether each cluster is free to scrub.
    free: Vec<bool>,
    has_hashes: bool,
    size: u64,
}

fn unreadable(what: &str) -> Error {
    Error::Corrupt(format!("cannot scrub: unreadable {what}"))
}

impl Scrubber {
    /// Find what a disc uses, failing if any of it cannot be read.
    pub fn new(volume: &mut Volume, source: &mut dyn Source) -> Result<Self> {
        let size = source.size();
        let mut scrubber = Self {
            free: vec![true; size.div_ceil(CLUSTER_SIZE) as usize],
            has_hashes: volume.has_hashes,
            size,
        };
        if volume.partitions.is_empty() {
            scrubber.parse_partition(volume, source, None)?;
            return Ok(scrubber);
        }
        // The header: mostly zeros anyway
        scrubber.mark(0, 0x50000);
        for partition in volume.partitions.clone() {
            let read = |source: &mut dyn Source, offset: u64, shifted: bool| -> Result<u64> {
                if shifted {
                    volume.read_shifted(source, partition + offset, None)
                } else {
                    let mut bytes = [0; 4];
                    source.read_at(partition + offset, &mut bytes)?;
                    Ok(u32::from_be_bytes(bytes) as u64)
                }
            };
            let fields = (|| -> Result<_> {
                Ok((
                    read(source, PARTITION_TMD_SIZE, false)?,
                    read(source, PARTITION_TMD_OFFSET, true)?,
                    read(source, PARTITION_CERT_CHAIN_SIZE, false)?,
                    read(source, PARTITION_CERT_CHAIN_OFFSET, true)?,
                    read(source, PARTITION_H3_OFFSET, true)?,
                ))
            })();
            let (tmd_size, tmd_offset, cert_size, cert_offset, h3_offset) =
                fields.map_err(|_| unreadable("partition header"))?;
            scrubber.mark(partition, 0x2C0);
            scrubber.mark(partition.saturating_add(tmd_offset), tmd_size);
            scrubber.mark(partition.saturating_add(cert_offset), cert_size);
            scrubber.mark(partition.saturating_add(h3_offset), PARTITION_H3_SIZE);
            scrubber.parse_partition(volume, source, Some(partition))?;
        }
        Ok(scrubber)
    }

    fn parse_partition(
        &mut self,
        volume: &mut Volume,
        source: &mut dyn Source,
        partition: Partition,
    ) -> Result<()> {
        let file_system = volume
            .file_system(source, partition)?
            .ok_or_else(|| unreadable("file system"))?;
        let data_offset = match partition {
            None => 0,
            Some(partition) => {
                partition
                    + volume
                        .read_shifted(source, partition + PARTITION_DATA_OFFSET, None)
                        .map_err(|_| unreadable("partition header"))?
            }
        };
        let mut read = |offset: u64| {
            volume
                .read_u32(source, offset, partition)
                .map_err(|_| unreadable("apploader"))
        };
        // The disc header, its information, and the apploader
        let apploader = 0x2440u32
            .wrapping_add(read(0x2440 + 0x14)?)
            .wrapping_add(read(0x2440 + 0x18)?);
        self.mark_in(data_offset, 0, apploader as u64);
        let dol = volume
            .dol_offset(source, partition)
            .ok()
            .flatten()
            .ok_or_else(|| unreadable("DOL"))?;
        let dol_size = volume
            .dol_size(source, partition, dol)
            .map_err(|_| unreadable("DOL"))?;
        self.mark_in(data_offset, dol, dol_size as u64);
        let (fst_offset, fst_size) = volume
            .fst(source, partition)
            .map_err(|_| unreadable("file system"))?;
        self.mark_in(data_offset, fst_offset, fst_size);
        for (offset, size) in file_system.files() {
            self.mark_in(data_offset, offset, size);
        }
        Ok(())
    }

    fn mark(&mut self, offset: u64, size: u64) {
        let mut current = offset / CLUSTER_SIZE * CLUSTER_SIZE;
        let end = offset.saturating_add(size);
        while current < end && current < self.size {
            self.free[(current / CLUSTER_SIZE) as usize] = false;
            current += CLUSTER_SIZE;
        }
    }

    /// Mark what lies at an offset into a partition's data, its hashes
    /// included.
    fn mark_in(&mut self, data_offset: u64, offset: u64, size: u64) {
        if data_offset == 0 {
            return self.mark(offset, size);
        }
        let first = self.cluster(offset).saturating_add(data_offset);
        let last = if size == 0 {
            first
        } else {
            self.cluster(offset.saturating_add(size - 1))
                .saturating_add(CLUSTER_SIZE + data_offset)
        };
        self.mark(first, last - first);
    }

    /// Where the cluster holding a partition offset starts.
    fn cluster(&self, offset: u64) -> u64 {
        if self.has_hashes {
            offset / 0x7C00 * CLUSTER_SIZE
        } else {
            offset / CLUSTER_SIZE * CLUSTER_SIZE
        }
    }

    /// Whether the cluster at `offset` is free to scrub.
    pub fn is_free(&self, offset: u64) -> bool {
        self.free
            .get((offset / CLUSTER_SIZE) as usize)
            .copied()
            .unwrap_or(true)
    }

    /// Zero what is free to scrub of `buf`, read from `offset`.
    pub fn scrub(&self, mut offset: u64, mut buf: &mut [u8]) {
        while !buf.is_empty() {
            let length = buf
                .len()
                .min((CLUSTER_SIZE - offset % CLUSTER_SIZE) as usize);
            if self.is_free(offset) {
                buf[..length].fill(0);
            }
            offset += length as u64;
            buf = &mut buf[length..];
        }
    }
}

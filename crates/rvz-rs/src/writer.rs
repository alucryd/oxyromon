//! Writing an RVZ: a port of Dolphin's `WIARVZFileReader::Convert`. Wii
//! partition data is stored decrypted and without its hashes, keeping only
//! those that recomputing them would not give back, and padding is stored as
//! the seed that generates it.
//!
//! Groups are processed in parallel and written in order, which gives what
//! Dolphin's own multithreaded writer gives.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::ops::Bound;
use std::path::Path;
use std::sync::Arc;

use rayon::prelude::*;
use sha1::{Digest, Sha1};

use crate::compression::{Compression, Compressor};
use crate::crypto::Key;
use crate::disc::{Iso, PARTITION_DATA_OFFSET, PARTITION_DATA_SIZE, Platform, Source, Volume};
use crate::error::{Error, Result};
use crate::format::*;
use crate::fst::FileSystem;
use crate::io;
use crate::lfg::{Lfg, SEED_BYTES, SEED_SIZE, Seed};
use crate::reader::Reader;
use crate::scrub::Scrubber;
use crate::wii::{
    self, BLOCK_DATA_SIZE, BLOCK_HEADER_SIZE, BLOCK_TOTAL_SIZE, BLOCKS_PER_GROUP, BlockData,
    DIGEST_SIZE, GROUP_DATA_SIZE, GROUP_TOTAL_SIZE, HashBlock,
};

/// How to write an RVZ.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub compression: Compression,
    /// The compression level, ignored without compression.
    pub level: i32,
    /// The size of the chunks data is compressed in: a power of two from 32
    /// KiB to 2 MiB, or a multiple of 2 MiB.
    pub chunk_size: u32,
    /// Zero what nothing on the disc points to, which no longer gives back
    /// the disc that went in.
    pub scrub: bool,
}

impl Default for Options {
    /// Dolphin's defaults: Zstandard at level 5, in 128 KiB chunks.
    fn default() -> Self {
        Self {
            compression: Compression::Zstd,
            level: 5,
            chunk_size: 128 * 1024,
            scrub: false,
        }
    }
}

/// Sizes of the two files a run touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub input_size: u64,
    pub output_size: u64,
}

impl Stats {
    /// Compressed size as a percentage of the original.
    pub fn ratio_percent(&self) -> f64 {
        if self.input_size == 0 {
            0.0
        } else {
            self.output_size as f64 * 100.0 / self.input_size as f64
        }
    }
}

/// Whether a chunk size is one RVZ allows.
pub fn is_valid_chunk_size(chunk_size: u32) -> bool {
    let chunk_size = chunk_size as u64;
    (chunk_size >= BLOCK_TOTAL_SIZE
        && chunk_size.is_power_of_two()
        && chunk_size <= GROUP_TOTAL_SIZE)
        || (chunk_size > 0 && chunk_size.is_multiple_of(GROUP_TOTAL_SIZE))
}

/// Whether `path` is an RVZ, by its magic.
pub fn is_rvz(path: &Path) -> Result<bool> {
    let file = File::open(path)?;
    let mut magic = [0; 4];
    Ok(io::read_exact_at(&file, &mut magic, 0).is_ok() && magic == MAGIC)
}

/// Write `input`, a raw GameCube or Wii disc or an RVZ, to `output` as an
/// RVZ. `keys` are the Wii common keys to decrypt a Wii disc's partitions
/// with; an RVZ keeps its title keys, and needs none.
///
/// `progress` is called with the input bytes consumed since the last call.
pub fn compress(
    input: &Path,
    output: &Path,
    options: &Options,
    keys: &[Key],
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    if !is_valid_chunk_size(options.chunk_size) {
        return Err(Error::InvalidOption(format!(
            "chunk size {} is neither a power of two from 32 KiB to 2 MiB nor a multiple of 2 MiB",
            options.chunk_size
        )));
    }
    let compressor = Compressor::new(options.compression, options.level)?;
    let input_size = std::fs::metadata(input)?.len();
    let mut source: Box<dyn Source> = if is_rvz(input)? {
        Box::new(Reader::open(input)?)
    } else {
        Box::new(Iso::open(input)?)
    };
    let part = io::part(output);
    let result = (|| {
        let mut file = File::create(&part)?;
        let mut writer = Writer::new(&mut *source, keys, options, compressor)?;
        writer.write(&mut file, input_size, progress)?;
        Ok(Stats {
            input_size,
            output_size: file.metadata()?.len(),
        })
    })();
    io::finish(&part, output, result)
}

#[derive(Clone, Copy)]
enum DataEntry {
    Raw(usize),
    Partition(usize, usize),
}

/// What makes two groups the same, so that the second can point to the
/// first's data.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ReuseId {
    key: Key,
    data_size: u64,
    encrypted: bool,
    value: u8,
}

#[derive(Default)]
struct OutputEntry {
    exception_lists: Vec<u8>,
    main_data: Vec<u8>,
    reuse_id: Option<ReuseId>,
    reused_group: Option<GroupEntry>,
    packed_size: u32,
    compressed: bool,
}

/// A run of input to process into one or more groups.
struct Job {
    data: Vec<u8>,
    entry: DataEntry,
    data_offset: u64,
    bytes_read: u64,
    group_index: usize,
}

/// What stays the same for every job.
struct Context<'a> {
    partitions: &'a [PartitionEntry],
    file_systems: &'a [Option<Arc<FileSystem>>],
    disc_file_system: Option<&'a FileSystem>,
    chunks_per_wii_group: u64,
    exception_lists_per_chunk: u64,
    compressed_exception_lists: bool,
    compressor: Compressor,
}

struct Writer<'a> {
    source: &'a mut dyn Source,
    volume: Option<Volume>,
    scrubber: Option<Scrubber>,
    options: Options,
    compressor: Compressor,
}

fn sha1(data: &[u8]) -> [u8; 20] {
    Sha1::digest(data).into()
}

fn all_same(data: &[u8]) -> bool {
    data.iter().all(|&byte| byte == data[0])
}

impl<'a> Writer<'a> {
    fn new(
        source: &'a mut dyn Source,
        keys: &[Key],
        options: &Options,
        compressor: Compressor,
    ) -> Result<Self> {
        let mut volume = Volume::new(source, keys)?;
        let scrubber = if options.scrub {
            let volume = volume.as_mut().ok_or_else(|| {
                Error::Unsupported("scrubbing what is not a GameCube or Wii disc".into())
            })?;
            Some(Scrubber::new(volume, source)?)
        } else {
            None
        };
        Ok(Self {
            source,
            volume,
            scrubber,
            options: *options,
            compressor,
        })
    }

    /// Read the input, scrubbed if scrubbing.
    fn read(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.source.read_at(offset, buf)?;
        if let Some(scrubber) = &self.scrubber {
            scrubber.scrub(offset, buf);
        }
        Ok(())
    }

    fn write(
        &mut self,
        file: &mut File,
        input_size: u64,
        progress: &mut dyn FnMut(u64),
    ) -> Result<()> {
        let iso_size = self.source.size();
        let chunk_size = self.options.chunk_size as u64;

        let mut layout = Layout::new(chunk_size);
        let mut file_systems = Vec::new();
        self.set_up_data_entries(iso_size, &mut layout, &mut file_systems)?;
        let disc_file_system = match &mut self.volume {
            Some(volume) => volume.file_system(self.source, None)?,
            None => None,
        };

        let mut group_entries = vec![GroupEntry::default(); layout.total_groups];
        let headers_upper_bound = {
            let mut bound = (HEADER_1_SIZE
                + HEADER_2_SIZE
                + layout.partitions.len() * PARTITION_ENTRY_SIZE
                + layout.raw_data.len() * RAW_DATA_ENTRY_SIZE
                + 0x100) as u64;
            let groups_size = (group_entries.len() * GROUP_ENTRY_SIZE) as u64;
            // Group entries usually compress to 9/16 of their size or better
            bound += if self.options.compression != Compression::None {
                groups_size * 9 / 16
            } else {
                groups_size
            };
            bound.next_multiple_of(BLOCK_TOTAL_SIZE)
        };
        file.write_all(&vec![0; headers_upper_bound as usize])?;
        let mut bytes_written = headers_upper_bound;

        let mut header_2 = Header2::default();
        let mut disc_header = [0; DISC_HEADER_SIZE];
        self.read(0, &mut disc_header)?;
        header_2.disc_header = disc_header;

        let context = Context {
            partitions: &layout.partitions,
            file_systems: &file_systems,
            disc_file_system: disc_file_system.as_deref(),
            chunks_per_wii_group: (GROUP_TOTAL_SIZE / chunk_size).max(1),
            exception_lists_per_chunk: (chunk_size / GROUP_TOTAL_SIZE).max(1),
            compressed_exception_lists: self.options.compression != Compression::None,
            compressor: self.compressor,
        };
        let mut reusable: BTreeMap<ReuseId, GroupEntry> = BTreeMap::new();

        let batch_size = rayon::current_num_threads() * 2;
        let mut jobs: Vec<Job> = Vec::with_capacity(batch_size);
        let mut bytes_read = 0u64;
        let mut reported = 0u64;
        let mut groups_processed = 0usize;
        let mut flush = |jobs: &mut Vec<Job>,
                         reusable: &mut BTreeMap<ReuseId, GroupEntry>,
                         group_entries: &mut [GroupEntry],
                         bytes_written: &mut u64,
                         file: &mut File|
         -> Result<()> {
            let processed: Vec<(Vec<OutputEntry>, u64, usize)> = {
                let reusable = &*reusable;
                jobs.par_drain(..)
                    .map(|job| {
                        let bytes_read = job.bytes_read;
                        let group_index = job.group_index;
                        process(&context, job, reusable)
                            .map(|entries| (entries, bytes_read, group_index))
                    })
                    .collect::<Result<_>>()?
            };
            for (mut entries, job_bytes_read, group_index) in processed {
                output(
                    &mut entries,
                    file,
                    reusable,
                    &mut group_entries[group_index..],
                    bytes_written,
                )?;
                // Scale an RVZ's progress from the disc to the file
                let consumed =
                    (job_bytes_read as u128 * input_size as u128 / iso_size.max(1) as u128) as u64;
                progress(consumed - reported);
                reported = consumed;
            }
            Ok(())
        };

        for data_entry in layout.data_entries.clone() {
            let (
                first_group,
                last_group,
                mut data_offset,
                mut data_size,
                mut data_offset_in_partition,
            ) = match data_entry {
                DataEntry::Partition(index, data_index) => {
                    let partition = &layout.partitions[index];
                    let data = partition.data[data_index];
                    let first_sector = data.first_sector as u64;
                    (
                        data.group_index as usize,
                        (data.group_index + data.groups) as usize,
                        first_sector * BLOCK_TOTAL_SIZE,
                        data.sectors as u64 * BLOCK_TOTAL_SIZE,
                        (first_sector - partition.data[0].first_sector as u64) * BLOCK_DATA_SIZE,
                    )
                }
                DataEntry::Raw(index) => {
                    let raw = layout.raw_data[index];
                    let skipped = raw.data_offset % BLOCK_TOTAL_SIZE;
                    let offset = raw.data_offset - skipped;
                    (
                        raw.group_index as usize,
                        (raw.group_index + raw.groups) as usize,
                        offset,
                        raw.data_size + skipped,
                        offset,
                    )
                }
            };
            if groups_processed != first_group || bytes_read != data_offset {
                return Err(Error::Corrupt("disc layout out of order".into()));
            }
            let is_partition = matches!(data_entry, DataEntry::Partition(..));
            while groups_processed < last_group {
                let mut length = chunk_size;
                if is_partition {
                    length = length.max(GROUP_TOTAL_SIZE);
                }
                length = length.min(data_offset + data_size - bytes_read);
                let mut data = vec![0; length as usize];
                self.read(bytes_read, &mut data)?;
                bytes_read += length;
                jobs.push(Job {
                    data,
                    entry: data_entry,
                    data_offset: data_offset_in_partition,
                    bytes_read,
                    group_index: groups_processed,
                });
                if jobs.len() == batch_size {
                    flush(
                        &mut jobs,
                        &mut reusable,
                        &mut group_entries,
                        &mut bytes_written,
                        file,
                    )?;
                }
                data_offset += length;
                data_size -= length;
                data_offset_in_partition += if is_partition {
                    length / BLOCK_TOTAL_SIZE * BLOCK_DATA_SIZE
                } else {
                    length
                };
                groups_processed += length.div_ceil(chunk_size) as usize;
            }
            if data_size != 0 {
                return Err(Error::Corrupt("disc layout out of order".into()));
            }
        }
        flush(
            &mut jobs,
            &mut reusable,
            &mut group_entries,
            &mut bytes_written,
            file,
        )?;
        drop(flush);
        if groups_processed != layout.total_groups || bytes_read != iso_size {
            return Err(Error::Corrupt("disc layout incomplete".into()));
        }

        // The tables, compressed as the groups are
        let (compressor_data_size, compressor_data) = self.compressor.header_data();
        let mut partition_entries = Vec::new();
        layout
            .partitions
            .iter()
            .for_each(|entry| entry.write(&mut partition_entries));
        let mut raw_data_entries = Vec::new();
        layout
            .raw_data
            .iter()
            .for_each(|entry| entry.write(&mut raw_data_entries));
        let mut group_bytes = Vec::new();
        group_entries
            .iter()
            .for_each(|entry| entry.write(&mut group_bytes));
        let raw_data_entries = self.compressor.compress(&[&raw_data_entries])?;
        let group_bytes = self.compressor.compress(&[&group_bytes])?;

        let mut bytes_written = (HEADER_1_SIZE + HEADER_2_SIZE) as u64;
        file.seek(SeekFrom::Start(bytes_written))?;
        let partition_entries_offset = write_header(
            file,
            &partition_entries,
            headers_upper_bound,
            &mut bytes_written,
        )?;
        let raw_data_entries_offset = write_header(
            file,
            &raw_data_entries,
            headers_upper_bound,
            &mut bytes_written,
        )?;
        let group_entries_offset =
            write_header(file, &group_bytes, headers_upper_bound, &mut bytes_written)?;

        header_2.disc_type = match self.volume.as_ref().map(|volume| volume.platform) {
            Some(Platform::GameCube) => 1,
            Some(Platform::Wii) => 2,
            None => 0,
        };
        header_2.compression_type = self.compressor.compression.id();
        header_2.compression_level = self.compressor.level;
        header_2.chunk_size = self.options.chunk_size;
        header_2.partition_entries = layout.partitions.len() as u32;
        header_2.partition_entry_size = PARTITION_ENTRY_SIZE as u32;
        header_2.partition_entries_offset = partition_entries_offset;
        header_2.partition_entries_hash = sha1(&partition_entries);
        header_2.raw_data_entries = layout.raw_data.len() as u32;
        header_2.raw_data_entries_offset = raw_data_entries_offset;
        header_2.raw_data_entries_size = raw_data_entries.len() as u32;
        header_2.group_entries = group_entries.len() as u32;
        header_2.group_entries_offset = group_entries_offset;
        header_2.group_entries_size = group_bytes.len() as u32;
        header_2.compressor_data_size = compressor_data_size;
        header_2.compressor_data = compressor_data;
        let header_2 = header_2.to_bytes();

        file.flush()?;
        let mut header_1 = Header1 {
            magic: MAGIC,
            version: VERSION,
            version_compatible: VERSION_WRITE_COMPATIBLE,
            header_2_size: HEADER_2_SIZE as u32,
            header_2_hash: sha1(&header_2),
            iso_file_size: iso_size,
            file_size: file.metadata()?.len(),
            header_1_hash: [0; 20],
        };
        header_1.header_1_hash = sha1(&header_1.to_bytes()[..HEADER_1_SIZE - DIGEST_SIZE]);
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&header_1.to_bytes())?;
        file.write_all(&header_2)?;
        file.sync_all()?;
        Ok(())
    }

    /// Split the disc into raw data and Wii partition data, the latter
    /// stored decrypted and without hashes.
    fn set_up_data_entries(
        &mut self,
        iso_size: u64,
        layout: &mut Layout,
        file_systems: &mut Vec<Option<Arc<FileSystem>>>,
    ) -> Result<()> {
        let partitions = match &self.volume {
            Some(volume) if volume.has_hashes && volume.has_encryption => volume.partitions.clone(),
            _ => Vec::new(),
        };
        let mut last_end = 0;
        for partition in partitions {
            // Anything odd about a partition is stored as raw data instead
            if partition < last_end {
                continue;
            }
            let mut signature = [0; 4];
            if self.source.read_at(partition, &mut signature).is_err()
                || u32::from_be_bytes(signature) != 0x10001
            {
                // Most likely garbage, as some WBFS tools leave off a scrubbed partition
                continue;
            }
            let volume = self.volume.as_mut().unwrap();
            let data_offset =
                volume.read_shifted(self.source, partition + PARTITION_DATA_OFFSET, None)?;
            let data_size =
                volume.read_shifted(self.source, partition + PARTITION_DATA_SIZE, None)?;
            let data_start = partition + data_offset;
            let data_end = data_start + data_size;
            if !data_start.is_multiple_of(BLOCK_TOTAL_SIZE) || data_size < BLOCK_TOTAL_SIZE {
                continue;
            }
            let (fst_offset, fst_size) = volume.fst(self.source, Some(partition))?;
            let key = volume.key(self.source, partition)?;

            layout.add_raw_data(last_end, partition - last_end);
            layout.add_raw_data(partition, data_offset);

            let fst_end = volume.to_raw_offset(self.source, fst_offset + fst_size, Some(partition));
            let split = fst_end
                .checked_sub(data_start)
                .and_then(|end| end.checked_next_multiple_of(GROUP_TOTAL_SIZE))
                .map_or(data_end, |end| data_end.min(end + data_start));

            let index = layout.partitions.len();
            let first = layout.partition_data(data_start, split - data_start, index, 0);
            let second = layout.partition_data(split, data_end - split, index, 1);
            last_end = (second.first_sector as u64 + second.sectors as u64) * BLOCK_TOTAL_SIZE;
            layout.partitions.push(PartitionEntry {
                key,
                data: [first, second],
            });
            file_systems.push(volume.file_system(self.source, Some(partition))?);
        }
        layout.add_raw_data(last_end, iso_size - last_end);
        Ok(())
    }
}

/// How the disc is split up, and into how many groups.
struct Layout {
    chunk_size: u64,
    total_groups: usize,
    partitions: Vec<PartitionEntry>,
    raw_data: Vec<RawDataEntry>,
    data_entries: Vec<DataEntry>,
}

impl Layout {
    fn new(chunk_size: u64) -> Self {
        Self {
            chunk_size,
            total_groups: 0,
            partitions: Vec::new(),
            raw_data: Vec::new(),
            data_entries: Vec::new(),
        }
    }

    fn add_raw_data(&mut self, mut offset: u64, mut size: u64) {
        // The disc header is kept in the second header
        let skip = if offset < DISC_HEADER_SIZE as u64 {
            (DISC_HEADER_SIZE as u64 - offset).min(size)
        } else {
            0
        };
        offset += skip;
        size -= skip;
        if size == 0 {
            return;
        }
        let groups = size.div_ceil(self.chunk_size) as u32;
        self.data_entries.push(DataEntry::Raw(self.raw_data.len()));
        self.raw_data.push(RawDataEntry {
            data_offset: offset,
            data_size: size,
            group_index: self.total_groups as u32,
            groups,
        });
        self.total_groups += groups as usize;
    }

    fn partition_data(
        &mut self,
        offset: u64,
        size: u64,
        index: usize,
        data_index: usize,
    ) -> PartitionDataEntry {
        let rounded = size / BLOCK_TOTAL_SIZE * BLOCK_TOTAL_SIZE;
        let groups = rounded.div_ceil(self.chunk_size) as u32;
        let entry = PartitionDataEntry {
            first_sector: (offset / BLOCK_TOTAL_SIZE) as u32,
            sectors: (size / BLOCK_TOTAL_SIZE) as u32,
            group_index: self.total_groups as u32,
            groups,
        };
        self.total_groups += groups as usize;
        self.data_entries
            .push(DataEntry::Partition(index, data_index));
        entry
    }
}

/// Write one of the tables the second header points to, at the end of the
/// file if it no longer fits the room left for them.
fn write_header(
    file: &mut File,
    data: &[u8],
    upper_bound: u64,
    bytes_written: &mut u64,
) -> Result<u64> {
    if *bytes_written <= upper_bound && *bytes_written + data.len() as u64 > upper_bound {
        *bytes_written = file.seek(SeekFrom::End(0))?;
    }
    let offset = *bytes_written;
    file.write_all(data)?;
    *bytes_written += data.len() as u64;
    pad_to_4(file, bytes_written)?;
    Ok(offset)
}

fn pad_to_4(file: &mut File, bytes_written: &mut u64) -> Result<()> {
    let padding = bytes_written.next_multiple_of(4) - *bytes_written;
    if padding > 0 {
        file.write_all(&[0; 4][..padding as usize])?;
        *bytes_written += padding;
    }
    Ok(())
}

fn try_reuse(reusable: &BTreeMap<ReuseId, GroupEntry>, entry: &mut OutputEntry) -> bool {
    if entry.reused_group.is_some() {
        return true;
    }
    let Some(id) = entry.reuse_id else {
        return false;
    };
    match reusable.get(&id) {
        Some(group) => {
            entry.reused_group = Some(*group);
            true
        }
        None => false,
    }
}

/// Process a job into groups ready to write: decrypted, packed, compressed.
fn process(
    context: &Context,
    job: Job,
    reusable: &BTreeMap<ReuseId, GroupEntry>,
) -> Result<Vec<OutputEntry>> {
    let mut entries: Vec<OutputEntry> = Vec::new();
    let compression = context.compressor.compression != Compression::None;

    match job.entry {
        DataEntry::Raw(_) => {
            let mut entry = OutputEntry::default();
            if all_same(&job.data) {
                entry.reuse_id = Some(ReuseId {
                    key: [0; 16],
                    data_size: job.data.len() as u64,
                    encrypted: false,
                    value: job.data[0],
                });
            }
            entries.push(entry);
            let size = job.data.len() as u64;
            rvz_pack(
                &job.data,
                &mut entries,
                size,
                1,
                size,
                job.data_offset,
                false,
                true,
                compression,
                context.disc_file_system,
            );
        }
        DataEntry::Partition(index, _) => {
            let partition = &context.partitions[index];
            let key = partition.key;
            let data = &job.data;
            let groups = (data.len() as u64).div_ceil(GROUP_TOTAL_SIZE);
            let blocks = data.len() as u64 / BLOCK_TOTAL_SIZE;
            let blocks_per_chunk = if context.chunks_per_wii_group == 1 {
                context.exception_lists_per_chunk * BLOCKS_PER_GROUP
            } else {
                BLOCKS_PER_GROUP / context.chunks_per_wii_group
            };
            let chunks = blocks.div_ceil(blocks_per_chunk);
            let in_per_chunk = blocks_per_chunk * BLOCK_TOTAL_SIZE;
            let out_per_chunk = blocks_per_chunk * BLOCK_DATA_SIZE;

            for i in 0..chunks {
                let block_index = i * blocks_per_chunk;
                let mut entry = OutputEntry::default();
                // Encrypted data all the same, as scrubbed data is, is stored once
                let start = (block_index * BLOCK_TOTAL_SIZE) as usize;
                let end = (start + in_per_chunk as usize).min(data.len());
                if all_same(&data[start..end]) {
                    entry.reuse_id = Some(ReuseId {
                        key,
                        data_size: (blocks - block_index).min(blocks_per_chunk) * BLOCK_DATA_SIZE,
                        encrypted: true,
                        value: data[0],
                    });
                }
                try_reuse(reusable, &mut entry);
                if entry.reused_group.is_none()
                    && let Some(id) = entry.reuse_id
                    && let Some(earlier) =
                        entries.iter().find(|earlier| earlier.reuse_id == Some(id))
                {
                    entry.reused_group = earlier.reused_group;
                }
                entries.push(entry);
            }

            if !entries.iter().all(|entry| entry.reused_group.is_some()) {
                let lists = if context.chunks_per_wii_group == 1 {
                    context.exception_lists_per_chunk
                } else {
                    chunks
                };
                let mut exception_lists: Vec<Vec<Exception>> = vec![Vec::new(); lists as usize];
                let mut decrypted: Vec<BlockData> =
                    vec![[0; BLOCK_DATA_SIZE as usize]; BLOCKS_PER_GROUP as usize];

                for i in 0..groups {
                    let group_offset = i * GROUP_TOTAL_SIZE;
                    let blocks_in_group = BLOCKS_PER_GROUP.min(blocks - i * BLOCKS_PER_GROUP);
                    decrypted.par_iter_mut().enumerate().for_each(|(j, out)| {
                        if (j as u64) < blocks_in_group {
                            let at = (group_offset + j as u64 * BLOCK_TOTAL_SIZE) as usize;
                            wii::decrypt_data(&key, &data[at..at + BLOCK_TOTAL_SIZE as usize], out);
                        } else {
                            out.fill(0);
                        }
                    });
                    let hashes = wii::hash_group(&decrypted);

                    for j in 0..blocks_in_group {
                        let chunk_index = (j / blocks_per_chunk) as usize;
                        let block_in_chunk = j % blocks_per_chunk;
                        if entries[chunk_index].reused_group.is_some() {
                            continue;
                        }
                        let list = if context.chunks_per_wii_group == 1 {
                            i as usize
                        } else {
                            chunk_index
                        };
                        let at = (group_offset + j * BLOCK_TOTAL_SIZE) as usize;
                        let desired =
                            wii::decrypt_hashes(&key, &data[at..at + BLOCK_TOTAL_SIZE as usize]);
                        let computed: &HashBlock = &hashes[j as usize];
                        // Keep a hash that recomputing would not give back,
                        // or every hash of a chunk that may be reused next to
                        // other chunks than these, which changes the hashes
                        let reusable_here = context.chunks_per_wii_group != 1
                            && entries[chunk_index].reuse_id.is_some();
                        for (offset, size) in [
                            wii::H0,
                            wii::PADDING_0,
                            wii::H1,
                            wii::PADDING_1,
                            wii::H2,
                            wii::PADDING_2,
                        ] {
                            for l in (0..size).step_by(DIGEST_SIZE) {
                                let at = offset + l.min(size - DIGEST_SIZE);
                                let hash = &desired[at..at + DIGEST_SIZE];
                                if reusable_here || hash != &computed[at..at + DIGEST_SIZE] {
                                    exception_lists[list].push(Exception {
                                        offset: (block_in_chunk * BLOCK_HEADER_SIZE) as u16
                                            + at as u16,
                                        hash: hash.try_into().unwrap(),
                                    });
                                }
                            }
                        }
                    }

                    // Junk is only stored as a seed where a reused chunk lands
                    // at the same offset into a block
                    let allow_junk_reuse = context.chunks_per_wii_group == 1;
                    let bytes_per_chunk = out_per_chunk.min(GROUP_DATA_SIZE);
                    let total_size = blocks_in_group * BLOCK_DATA_SIZE;
                    let flat: &[u8] = decrypted.as_flattened();
                    rvz_pack(
                        &flat[..total_size as usize],
                        &mut entries,
                        bytes_per_chunk,
                        chunks as usize,
                        total_size,
                        job.data_offset + i * GROUP_DATA_SIZE,
                        groups > 1,
                        allow_junk_reuse,
                        compression,
                        context.file_systems[index].as_deref(),
                    );
                }

                for (i, list) in exception_lists.iter().enumerate() {
                    let entry = &mut entries[if context.chunks_per_wii_group == 1 {
                        0
                    } else {
                        i
                    }];
                    if entry.reused_group.is_some() {
                        continue;
                    }
                    entry
                        .exception_lists
                        .extend_from_slice(&(list.len() as u16).to_be_bytes());
                    list.iter()
                        .for_each(|exception| exception.write(&mut entry.exception_lists));
                }
                for entry in &mut entries {
                    // Decrypted data all the same is only reused without exceptions
                    if let Some(id) = entry.reuse_id
                        && !id.encrypted
                        && entry.exception_lists.iter().any(|&byte| byte != 0)
                    {
                        entry.reuse_id = None;
                    }
                }
            }
        }
    }

    for entry in &mut entries {
        if try_reuse(reusable, entry) {
            continue;
        }
        // A group of zeros is stored as a size of zero
        if let Some(id) = entry.reuse_id
            && !id.encrypted
            && id.value == 0
        {
            entry.exception_lists.clear();
            entry.main_data.clear();
            entry.packed_size = 0;
            entry.compressed = false;
            continue;
        }
        let mut compressed = None;
        if compression {
            let parts: Vec<&[u8]> =
                if context.compressed_exception_lists && !entry.exception_lists.is_empty() {
                    vec![&entry.exception_lists, &entry.main_data]
                } else {
                    vec![&entry.main_data]
                };
            let packed = context.compressor.compress(&parts)?;
            let mut uncompressed_size = entry.main_data.len();
            if context.compressed_exception_lists {
                uncompressed_size += entry.exception_lists.len().next_multiple_of(4);
            }
            if packed.len() < uncompressed_size {
                compressed = Some(packed);
            }
        }
        entry.compressed = compressed.is_some();
        match compressed {
            Some(packed) => {
                entry.main_data = packed;
                if context.compressed_exception_lists {
                    entry.exception_lists.clear();
                }
            }
            None => {
                while !entry.exception_lists.len().is_multiple_of(4) {
                    entry.exception_lists.push(0);
                }
            }
        }
    }
    Ok(entries)
}

/// Write processed groups, or point them to an identical one written
/// earlier.
fn output(
    entries: &mut [OutputEntry],
    file: &mut File,
    reusable: &mut BTreeMap<ReuseId, GroupEntry>,
    group_entries: &mut [GroupEntry],
    bytes_written: &mut u64,
) -> Result<()> {
    for (entry, group_entry) in entries.iter_mut().zip(group_entries.iter_mut()) {
        if try_reuse(reusable, entry) {
            *group_entry = entry.reused_group.unwrap();
            continue;
        }
        if *bytes_written >> 2 > u32::MAX as u64 {
            return Err(Error::Unsupported("an RVZ of 16 GiB or more".into()));
        }
        let size = (entry.exception_lists.len() + entry.main_data.len()) as u32;
        *group_entry = GroupEntry {
            data_offset: (*bytes_written >> 2) as u32,
            data_size: (size & 0x7FFF_FFFF) | ((entry.compressed as u32) << 31),
            packed_size: entry.packed_size,
        };
        file.write_all(&entry.exception_lists)?;
        file.write_all(&entry.main_data)?;
        *bytes_written += size as u64;
        if let Some(id) = entry.reuse_id {
            reusable.entry(id).or_insert(*group_entry);
        }
        pad_to_4(file, bytes_written)?;
    }
    Ok(())
}

/// Pack data as RVZ stores it: runs of data, and runs of junk as the seed
/// that generates them. `data` is `chunks` chunks of `bytes_per_chunk`,
/// `total_size` in all, at `data_offset`.
#[allow(clippy::too_many_arguments)]
fn rvz_pack(
    data: &[u8],
    out: &mut [OutputEntry],
    bytes_per_chunk: u64,
    chunks: usize,
    total_size: u64,
    mut data_offset: u64,
    multipart: bool,
    allow_junk_reuse: bool,
    compression: bool,
    file_system: Option<&FileSystem>,
) {
    // Where each run of junk ends, to where it starts and its seed
    let mut junk: BTreeMap<u64, (u64, Seed)> = BTreeMap::new();
    let total = total_size as usize;
    let mut position = 0usize;
    while position < total {
        // Skip the zeros that usually follow a file
        let zeros = data[position..total]
            .iter()
            .take_while(|&&byte| byte == 0)
            .count();
        // Many of them, as scrubbed junk leaves, are better stored as junk,
        // unless a compressor will do better with them
        if !compression && zeros > SEED_BYTES {
            junk.entry((position + zeros) as u64)
                .or_insert((position as u64, [0; SEED_SIZE]));
        }
        position += zeros;
        data_offset += zeros as u64;

        let length = ((data_offset + 1).next_multiple_of(BLOCK_TOTAL_SIZE) - data_offset)
            .min((total - position) as u64) as usize;
        let mut seed = [0; SEED_SIZE];
        let reconstructed = Lfg::seed(
            &data[position..position + length],
            (data_offset % BLOCK_TOTAL_SIZE) as usize,
            &mut seed,
        );
        if reconstructed > 0 {
            junk.entry((position + reconstructed) as u64)
                .or_insert((position as u64, seed));
        }
        // At a file with room left in the block after it, carry on after the
        // file rather than at the next block
        if let Some(file_system) = file_system
            && let Some((offset, size)) = file_system.file_at(data_offset + reconstructed as u64)
        {
            let file_end = offset + size;
            if file_end < data_offset + length as u64 {
                position += (file_end - data_offset) as usize;
                data_offset = file_end;
                continue;
            }
        }
        position += length;
        data_offset += length as u64;
    }

    for (i, entry) in out.iter_mut().enumerate().take(chunks) {
        if entry.reused_group.is_some() {
            continue;
        }
        let mut current = i as u64 * bytes_per_chunk;
        let end = (current + bytes_per_chunk).min(total_size);
        let store_junk_efficiently = allow_junk_reuse || entry.reuse_id.is_none();
        let mut first_iteration = !multipart;
        while current < end {
            let mut next_junk_start = end;
            let mut next_junk_end = end;
            let mut seed = None;
            if store_junk_efficiently
                && end - current > SEED_BYTES as u64
                && let Some((&junk_end, &(junk_start, junk_seed))) = junk
                    .range((
                        Bound::Excluded(current + SEED_BYTES as u64),
                        Bound::Unbounded,
                    ))
                    .next()
                && junk_start + (SEED_BYTES as u64) < end
            {
                next_junk_start = current.max(junk_start);
                next_junk_end = end.min(junk_end);
                seed = Some(junk_seed);
            }
            if first_iteration {
                if next_junk_start == end {
                    // Packing would only cost space
                    entry
                        .main_data
                        .extend_from_slice(&data[current as usize..end as usize]);
                    break;
                }
                first_iteration = false;
            }
            let data_bytes = next_junk_start - current;
            if data_bytes > 0 {
                entry
                    .main_data
                    .extend_from_slice(&(data_bytes as u32).to_be_bytes());
                entry
                    .main_data
                    .extend_from_slice(&data[current as usize..next_junk_start as usize]);
                current += data_bytes;
                entry.packed_size += 4 + data_bytes as u32;
            }
            let junk_bytes = next_junk_end - current;
            if junk_bytes > 0 {
                entry
                    .main_data
                    .extend_from_slice(&(junk_bytes as u32 | 0x8000_0000).to_be_bytes());
                for word in seed.unwrap() {
                    entry.main_data.extend_from_slice(&word.to_be_bytes());
                }
                current += junk_bytes;
                entry.packed_size += (4 + SEED_BYTES) as u32;
            }
        }
    }
}

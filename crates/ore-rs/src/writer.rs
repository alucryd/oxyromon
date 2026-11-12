//! Writing an ORE from a disc image.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;

use md5::Md5;
use rayon::prelude::*;
use sha1::{Digest, Sha1};

use super::cdc;
use super::codec::{self, LZMA2_PRESET, ZSTD_LEVEL};
use super::crypto::Key;
use super::disc::{Iso, PARTITION_DATA_SIZE, Platform, Source, Volume};
use super::error::{Error, Result};
use super::format::*;
use super::fst::FileSystem;
use super::io;
use super::segment::{BLOCK, Run, Segmenter};
use super::wii::{
    self, BLOCK_DATA_SIZE, BLOCK_TOTAL_SIZE, BLOCKS_PER_GROUP, BlockData, GROUP_DATA_SIZE,
    GROUP_TOTAL_SIZE, HashBlock,
};

/// How an ORE is written.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    pub profile: Profile,
    pub codec: Codec,
}

/// Sizes of the two files a run touched.
#[derive(Debug, Clone, Copy)]
pub struct Stats {
    pub input_size: u64,
    pub output_size: u64,
}

impl Stats {
    /// The output's size as a percentage of the input's.
    pub fn ratio_percent(&self) -> f64 {
        if self.input_size == 0 {
            0.0
        } else {
            self.output_size as f64 * 100.0 / self.input_size as f64
        }
    }
}

/// Whether `path` starts as an ORE does.
pub fn is_ore(path: &Path) -> Result<bool> {
    let file = File::open(path)?;
    let mut magic = [0; 4];
    Ok(io::read_exact_at(&file, &mut magic, 0).is_ok() && magic == MAGIC)
}

/// How much of a raw area is read at a time.
const WINDOW: u64 = 4 << 20;

/// An area of the disc, as the encoder plans to cut it.
struct Plan {
    start: u64,
    len: u64,
    kind: AreaKind,
    file_system: Option<Arc<FileSystem>>,
    key: Option<Key>,
}

/// The disc cut into areas: a GameCube disc whole; a Wii disc's partitions'
/// data apart, and what lies between them as it is.
fn plan(iso: &mut Iso, volume: &mut Volume) -> Result<Vec<Plan>> {
    let size = iso.size();
    if volume.platform == Platform::GameCube {
        return Ok(vec![Plan {
            start: 0,
            len: size,
            kind: AreaKind::Raw,
            file_system: volume.file_system(iso, None)?,
            key: None,
        }]);
    }
    let mut plans = Vec::new();
    let mut at = 0;
    if volume.has_hashes && volume.has_encryption {
        for partition in volume.partitions.clone() {
            let data_start = partition + volume.data_offset(iso, partition);
            let Ok(data_size) = volume.read_shifted(iso, partition + PARTITION_DATA_SIZE, None)
            else {
                continue;
            };
            // A partition the disc does not hold whole stays as it is
            if data_start <= partition
                || data_start < at
                || data_size == 0
                || !data_size.is_multiple_of(BLOCK_TOTAL_SIZE)
                || data_start + data_size > size
            {
                continue;
            }
            if data_start > at {
                plans.push(raw(at, data_start - at));
            }
            let key = volume.key(iso, partition)?;
            plans.push(Plan {
                start: data_start,
                len: data_size,
                kind: AreaKind::WiiPartition,
                file_system: volume.file_system(iso, Some(partition))?,
                key: Some(key),
            });
            at = data_start + data_size;
        }
    }
    if size > at {
        plans.push(raw(at, size - at));
    }
    Ok(plans)
}

fn raw(start: u64, len: u64) -> Plan {
    Plan {
        start,
        len,
        kind: AreaKind::Raw,
        file_system: None,
        key: None,
    }
}

/// The identity hashes of the disc, fed as it is read.
#[derive(Default)]
struct Hashes {
    crc32: crc32fast::Hasher,
    md5: Md5,
    sha1: Sha1,
    blake3: blake3::Hasher,
}

impl Hashes {
    fn update(&mut self, data: &[u8]) {
        let Self {
            crc32,
            md5,
            sha1,
            blake3,
        } = self;
        rayon::join(
            || {
                crc32.update(data);
                md5.update(data);
            },
            || {
                sha1.update(data);
                blake3.update(data);
            },
        );
    }
}

/// Stored chunks, deduped and gathered into groups, written as they fill.
struct Encoder {
    file: BufWriter<File>,
    codec: Codec,
    group_size: usize,
    index: Vec<u8>,
    records: u64,
    record_max: u64,
    seen: HashMap<[u8; 32], u64>,
    stored: u64,
    group: Vec<u8>,
    group_chunks: u64,
    /// Groups closed but not yet written.
    closed: Vec<(Vec<u8>, u64)>,
    groups: Vec<Group>,
    /// Bytes of stored chunks in the groups written.
    written: u64,
    padding: u64,
    zeros: u64,
    deduped: u64,
}

impl Encoder {
    fn push(&mut self, run: Run) {
        let length = match run {
            Run::Zero(n) => {
                self.index.push(KIND_ZERO);
                put_uvarint(&mut self.index, n);
                self.zeros += n;
                n
            }
            Run::Padding(n, seed) => {
                self.index.push(KIND_GENERATED);
                put_uvarint(&mut self.index, n);
                self.index.push(GEN_GC_WII_FILLER);
                self.index.extend_from_slice(&seed);
                self.padding += n;
                n
            }
            Run::Chunk(bytes) => {
                let n = bytes.len() as u64;
                let digest: [u8; 32] = blake3::hash(bytes).into();
                if let Some(&target) = self.seen.get(&digest) {
                    self.index.push(KIND_DEDUP);
                    put_uvarint(&mut self.index, n);
                    put_uvarint(&mut self.index, target);
                    self.deduped += n;
                } else {
                    self.seen.insert(digest, self.stored);
                    self.stored += 1;
                    self.index.push(KIND_SOLID);
                    put_uvarint(&mut self.index, n);
                    if self.group.len() + bytes.len() > self.group_size {
                        self.close_group();
                    }
                    self.group.extend_from_slice(bytes);
                    self.group_chunks += 1;
                }
                n
            }
        };
        self.records += 1;
        self.record_max = self.record_max.max(length);
    }

    fn close_group(&mut self) {
        if self.group_chunks > 0 {
            let group = std::mem::replace(&mut self.group, Vec::with_capacity(self.group_size));
            self.closed.push((group, self.group_chunks));
            self.group_chunks = 0;
        }
    }

    /// Compress and write the groups closed, once there are enough to keep
    /// every core busy, or all of them at the end.
    fn write_closed(&mut self, all: bool) -> Result<()> {
        if self.closed.is_empty() || (!all && self.closed.len() < rayon::current_num_threads()) {
            return Ok(());
        }
        let (codec, group_size) = (self.codec, self.group_size);
        let compressed: Vec<(Vec<u8>, u32)> = self
            .closed
            .par_iter()
            .map(|(data, _)| {
                Ok((
                    codec::compress(codec, data, group_size)?,
                    crc32c::crc32c(data),
                ))
            })
            .collect::<Result<_>>()?;
        for ((data, chunks), (stored, crc)) in self.closed.drain(..).zip(compressed) {
            let stored_len = stored.len() as u64;
            self.file
                .write_all(&envelope(codec, self.written, stored_len, crc))?;
            self.file.write_all(&stored)?;
            self.groups.push(Group {
                codec,
                chunks,
                logical_len: data.len() as u64,
                stored_len,
            });
            self.written += data.len() as u64;
        }
        Ok(())
    }
}

/// Write `input`, a raw GameCube or Wii disc, to `output` as an ORE. `keys`
/// are the Wii common keys to decrypt a Wii disc's partitions with.
///
/// `progress` is called with the input bytes consumed since the last call.
pub fn compress(
    input: &Path,
    output: &Path,
    options: &Options,
    keys: &[Key],
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    let mut iso = Iso::open(input)?;
    let size = iso.size();
    let Some(mut volume) = Volume::new(&mut iso, keys)? else {
        return Err(Error::Unsupported(
            "neither a GameCube nor a Wii disc".into(),
        ));
    };
    let plans = plan(&mut iso, &mut volume)?;
    let part = io::part(output);
    let result = (|| {
        let mut file = BufWriter::with_capacity(8 << 20, File::create(&part)?);
        file.write_all(&[0; HEADER_LEN])?;
        let mut encoder = Encoder {
            file,
            codec: options.codec,
            group_size: options.profile.group_size(),
            index: Vec::new(),
            records: 0,
            record_max: 0,
            seen: HashMap::new(),
            stored: 0,
            group: Vec::with_capacity(options.profile.group_size()),
            group_chunks: 0,
            closed: Vec::new(),
            groups: Vec::new(),
            written: 0,
            padding: 0,
            zeros: 0,
            deduped: 0,
        };
        let mut hashes = Hashes::default();
        let mut areas = Vec::new();
        let mut exception_count = 0;
        for plan in &plans {
            let records = encoder.records;
            let fs = plan.file_system.as_deref();
            let mut exceptions = Vec::new();
            match plan.kind {
                AreaKind::Raw => {
                    let mut segmenter = Segmenter::new(plan.start);
                    let mut buf = vec![0; (WINDOW + BLOCK) as usize];
                    let mut at = 0;
                    while at < plan.len {
                        // Windows end on blocks, as the generator counts them
                        let end = (plan.start + at + WINDOW).next_multiple_of(BLOCK) - plan.start;
                        let length = (end.min(plan.len) - at) as usize;
                        let buf = &mut buf[..length];
                        iso.read_at(plan.start + at, buf)?;
                        hashes.update(buf);
                        segmenter.feed(buf, at, fs, &mut |run| encoder.push(run));
                        encoder.write_closed(false)?;
                        progress(length as u64);
                        at += length as u64;
                    }
                    segmenter.finish(&mut |run| encoder.push(run));
                }
                AreaKind::WiiPartition => {
                    let key = plan.key.expect("a partition has a key");
                    // A group's data is 62 blocks of padding: fed whole, it
                    // ends on one
                    let mut segmenter = Segmenter::new(0);
                    let blocks = plan.len / BLOCK_TOTAL_SIZE;
                    let mut raw = vec![0; GROUP_TOTAL_SIZE as usize];
                    for group in 0..blocks.div_ceil(BLOCKS_PER_GROUP) {
                        let count = (blocks - group * BLOCKS_PER_GROUP).min(BLOCKS_PER_GROUP);
                        let raw = &mut raw[..(count * BLOCK_TOTAL_SIZE) as usize];
                        iso.read_at(plan.start + group * GROUP_TOTAL_SIZE, raw)?;
                        hashes.update(raw);
                        let data = decrypt_group(&key, raw, group, &mut exceptions)?;
                        segmenter.feed(&data, group * GROUP_DATA_SIZE, fs, &mut |run| {
                            encoder.push(run)
                        });
                        encoder.write_closed(false)?;
                        progress(raw.len() as u64);
                    }
                    segmenter.finish(&mut |run| encoder.push(run));
                }
            }
            exception_count += exceptions.len() as u64;
            areas.push(Area {
                len: plan.len,
                kind: plan.kind,
                start: plan.start,
                records: encoder.records - records,
                title_key: plan.key.map(|key| key.to_vec()),
                exceptions,
            });
        }
        encoder.close_group();
        encoder.write_closed(true)?;

        // The index: records, then the group table
        let mut index = std::mem::take(&mut encoder.index);
        put_uvarint(&mut index, encoder.groups.len() as u64);
        for group in &encoder.groups {
            index.push(group.codec.id());
            put_uvarint(&mut index, group.chunks);
            put_uvarint(&mut index, group.logical_len);
            put_uvarint(&mut index, group.stored_len);
        }
        let mut file = encoder.file;
        let index_offset = file.stream_position()?;
        file.write_all(&index)?;

        let params = Params {
            codec: options.codec,
            group: options.profile.group_size() as u64,
            level: match options.codec {
                Codec::Zstd => ZSTD_LEVEL as u32,
                Codec::Lzma2 => LZMA2_PRESET,
            },
            chunker: Chunker {
                max: cdc::MAX as u64,
                min: cdc::MIN as u64,
                name: "mincdc-hash4".into(),
            },
        };
        let params_digest: [u8; 8] = blake3::hash(&to_cbor(&params)).as_bytes()[..8]
            .try_into()
            .unwrap();
        let Hashes {
            crc32,
            md5,
            sha1,
            blake3: logical,
        } = hashes;
        let logical_hash: [u8; 32] = logical.finalize().into();
        let wii = volume.platform == Platform::Wii;
        let mut ledger = Vec::new();
        if areas.iter().any(|area| area.kind == AreaKind::WiiPartition) {
            ledger.push(step("wii-decrypt", None));
            ledger.push(step("wii-hashes", Some(exception_count)));
        }
        ledger.push(step("gc-wii-filler-prng", Some(encoder.padding)));
        ledger.push(step("zero", Some(encoder.zeros)));
        ledger.push(step("mincdc-dedup", Some(encoder.deduped)));
        let identity = |algo: &str, value: Vec<u8>| Identity {
            algo: algo.into(),
            view: "logical".into(),
            value,
        };
        let meta = Meta {
            media: Media {
                size,
                areas,
                class: MEDIA_DVD_2048,
                platform: if wii { "wii" } else { "gamecube" }.into(),
            },
            ledger,
            identity: vec![
                identity("blake3-256", logical_hash.to_vec()),
                identity("sha-1", sha1.finalize().to_vec()),
                identity("md5", md5.finalize().to_vec()),
                identity("crc-32", crc32.finalize().to_be_bytes().to_vec()),
            ],
            provenance: Provenance {
                tool: concat!("ore-rs ", env!("CARGO_PKG_VERSION")).into(),
                params,
            },
        };
        let meta = to_cbor(&meta);
        let meta_offset = index_offset + index.len() as u64;
        file.write_all(&meta)?;
        let output_size = meta_offset + meta.len() as u64;

        let header = Header {
            flags_required: REQ_VARIABLE_CHUNKS | REQ_SOLID_GROUPS,
            profile: options.profile.id(),
            media_class: MEDIA_DVD_2048,
            logical_size: size,
            chunk_max: encoder.record_max as u32,
            chunk_count: u32::try_from(encoder.records)
                .map_err(|_| Error::Unsupported("more than 2^32 index records".into()))?,
            object_count: 1,
            index_offset,
            index_len: index.len() as u64,
            index_crc: crc32c::crc32c(&index),
            meta_offset,
            meta_len: meta.len() as u32,
            meta_crc: crc32c::crc32c(&meta),
            logical_hash,
            params_digest,
            ..Header::default()
        };
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&header.write())?;
        let file = file.into_inner().map_err(|e| e.into_error())?;
        file.sync_all()?;
        Ok(Stats {
            input_size: size,
            output_size,
        })
    })();
    io::finish(&part, output, result)
}

fn step(id: &str, bytes: Option<u64>) -> Step {
    Step {
        id: id.into(),
        bytes,
        verified: true,
    }
}

/// A Wii group's data, decrypted, its hashes checked against what
/// recomputing gives and those that differ kept as exceptions, and the whole
/// checked to encrypt back to `raw`.
fn decrypt_group(
    key: &Key,
    raw: &[u8],
    group: u64,
    exceptions: &mut Vec<Exception>,
) -> Result<Vec<u8>> {
    let (stored, mut data): (Vec<HashBlock>, Vec<BlockData>) = raw
        .par_chunks(BLOCK_TOTAL_SIZE as usize)
        .map(|block| {
            let mut data = [0; BLOCK_DATA_SIZE as usize];
            wii::decrypt_data(key, block, &mut data);
            (wii::decrypt_hashes(key, block), data)
        })
        .unzip();
    let count = data.len();
    data.resize(BLOCKS_PER_GROUP as usize, [0; BLOCK_DATA_SIZE as usize]);
    let mut hashes = wii::hash_group(&data);
    for (j, (stored, computed)) in stored.iter().zip(&mut hashes).enumerate() {
        if stored != computed {
            exceptions.push(Exception {
                block: group * BLOCKS_PER_GROUP + j as u64,
                hashes: stored.to_vec(),
            });
            *computed = *stored;
        }
    }
    let mut encrypted = vec![0; raw.len()];
    wii::encrypt_group(key, &hashes[..count], &data[..count], &mut encrypted);
    if encrypted != raw {
        return Err(Error::Corrupt(format!(
            "Wii group {group} does not encrypt back to what the disc holds"
        )));
    }
    Ok(data[..count].concat())
}

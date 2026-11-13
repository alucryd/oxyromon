//! Writing an ORE from a disc image.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;

use md5::Md5;
use rayon::prelude::*;
use sha1::{Digest, Sha1};

use super::cd::cue::{self, Mode};
use super::cd::sector::{LEAD_IN, SECTOR, data_range};
use super::cdc;
use super::cdtrack::MapWriter;
use super::codec::{FLAC_LEVEL, LZMA2_PRESET, ZSTD_LEVEL};
use super::crypto::Key;
use super::disc::{Iso, PARTITION_DATA_SIZE, Platform, Source, Volume};
use super::error::{Error, Result};
use super::format::*;
use super::fst::FileSystem;
use super::io;
use super::pack::Packer;
use super::reader::{Event, Reader};
use super::segment::{BLOCK, Run, Segmenter};
use super::store::StoreWriter;
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

/// Where an encoder's stored chunks go.
enum Sink {
    /// Into groups in the ORE itself, each chunk once.
    Inline {
        packer: Packer,
        seen: HashMap<[u8; 32], u64>,
        stored: u64,
        groups: Vec<Group>,
        /// Bytes of stored chunks in the groups written.
        written: u64,
    },
    /// Into a store, the ORE naming them by their ids there.
    Store {
        writer: StoreWriter,
        last: u64,
        used: HashSet<u64>,
    },
}

/// An ORE being written: its index records as runs come, its stored chunks
/// to its sink.
struct Encoder {
    file: BufWriter<File>,
    index: Vec<u8>,
    records: u64,
    record_max: u64,
    sink: Sink,
    padding: u64,
    zeros: u64,
    deduped: u64,
}

impl Encoder {
    fn new(part: &Path, options: &Options, store: Option<StoreWriter>) -> Result<Self> {
        let mut file = BufWriter::with_capacity(8 << 20, File::create(part)?);
        file.write_all(&[0; HEADER_LEN])?;
        let sink = match store {
            Some(writer) => Sink::Store {
                writer,
                last: 0,
                used: HashSet::new(),
            },
            None => Sink::Inline {
                packer: Packer::new(options.codec, options.profile.group_size()),
                seen: HashMap::new(),
                stored: 0,
                groups: Vec::new(),
                written: 0,
            },
        };
        Ok(Self {
            file,
            index: Vec::new(),
            records: 0,
            record_max: 0,
            sink,
            padding: 0,
            zeros: 0,
            deduped: 0,
        })
    }

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
                match &mut self.sink {
                    Sink::Inline {
                        packer,
                        seen,
                        stored,
                        ..
                    } => {
                        if let Some(&target) = seen.get(&digest) {
                            self.index.push(KIND_DEDUP);
                            put_uvarint(&mut self.index, n);
                            put_uvarint(&mut self.index, target);
                            self.deduped += n;
                        } else {
                            seen.insert(digest, *stored);
                            *stored += 1;
                            self.index.push(KIND_SOLID);
                            put_uvarint(&mut self.index, n);
                            packer.push(bytes);
                        }
                    }
                    Sink::Store { writer, last, used } => {
                        let id = writer.add(digest[..16].try_into().unwrap(), bytes);
                        if !used.insert(id) {
                            self.deduped += n;
                        }
                        self.index.push(KIND_STORE);
                        put_uvarint(&mut self.index, n);
                        put_uvarint(&mut self.index, zigzag(id as i64 - *last as i64));
                        *last = id;
                    }
                }
                n
            }
        };
        self.records += 1;
        self.record_max = self.record_max.max(length);
    }

    /// Compress what comes next with `codec`, the groups so far written
    /// first: a CD's audio tracks are FLAC, its data tracks not.
    fn set_codec(&mut self, codec: Codec) -> Result<()> {
        let current = match &self.sink {
            Sink::Inline { packer, .. } => packer.codec,
            Sink::Store { writer, .. } => writer.codec(),
        };
        if current != codec {
            self.flush(true)?;
            match &mut self.sink {
                Sink::Inline { packer, .. } => packer.set_codec(codec),
                Sink::Store { writer, .. } => writer.set_codec(codec)?,
            }
        }
        Ok(())
    }

    /// Write the groups filled, once there are enough to keep every core
    /// busy, or with `all` every one.
    fn flush(&mut self, all: bool) -> Result<()> {
        match &mut self.sink {
            Sink::Inline {
                packer,
                groups,
                written,
                ..
            } => {
                for packed in packer.take(all)? {
                    let group = packed.group;
                    self.file.write_all(&envelope(
                        group.codec,
                        *written,
                        group.stored_len,
                        packed.crc,
                    ))?;
                    self.file.write_all(&packed.stored)?;
                    groups.push(group);
                    *written += group.logical_len;
                }
                Ok(())
            }
            Sink::Store { writer, .. } => writer.write(all),
        }
    }

    /// Write the index, `meta` and the header, then commit the store the
    /// chunks went to, if any. Returns the ORE's size.
    fn finish(mut self, mut meta: Meta, options: &Options, logical_hash: [u8; 32]) -> Result<u64> {
        self.flush(true)?;
        let mut index = std::mem::take(&mut self.index);
        let (flags, index_kind) = match &self.sink {
            Sink::Inline { groups, .. } => {
                put_uvarint(&mut index, groups.len() as u64);
                for group in groups {
                    index.push(group.codec.id());
                    put_uvarint(&mut index, group.chunks);
                    put_uvarint(&mut index, group.logical_len);
                    put_uvarint(&mut index, group.stored_len);
                }
                meta.store = None;
                (REQ_VARIABLE_CHUNKS | REQ_SOLID_GROUPS, INDEX_FLAT)
            }
            Sink::Store { writer, .. } => {
                put_uvarint(&mut index, 0);
                meta.store = Some(StoreRef {
                    uuid: writer.uuid().to_vec(),
                });
                (REQ_VARIABLE_CHUNKS | REQ_LIBRARY, INDEX_LIBRARY)
            }
        };
        let params = Params {
            codec: options.codec,
            group: options.profile.group_size() as u64,
            level: match options.codec {
                Codec::Zstd => ZSTD_LEVEL as u32,
                Codec::Lzma2 => LZMA2_PRESET,
                Codec::Flac => FLAC_LEVEL,
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
        meta.provenance = Provenance {
            tool: concat!("ore-rs ", env!("CARGO_PKG_VERSION")).into(),
            params,
        };

        let mut file = self.file;
        let index_offset = file.stream_position()?;
        file.write_all(&index)?;
        let meta_bytes = to_cbor(&meta);
        let meta_offset = index_offset + index.len() as u64;
        file.write_all(&meta_bytes)?;
        let header = Header {
            flags_required: flags,
            profile: options.profile.id(),
            media_class: meta.media.class,
            index_kind,
            logical_size: meta.media.size,
            chunk_max: self.record_max as u32,
            chunk_count: u32::try_from(self.records)
                .map_err(|_| Error::Unsupported("more than 2^32 index records".into()))?,
            object_count: 1,
            index_offset,
            index_len: index.len() as u64,
            index_crc: crc32c::crc32c(&index),
            meta_offset,
            meta_len: meta_bytes.len() as u32,
            meta_crc: crc32c::crc32c(&meta_bytes),
            logical_hash,
            params_digest,
            ..Header::default()
        };
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&header.write())?;
        file.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        // The store is committed once the ORE naming its chunks is whole
        if let Sink::Store {
            mut writer, used, ..
        } = self.sink
        {
            for id in used {
                writer.reference(id);
            }
            writer.commit()?;
        }
        Ok(meta_offset + meta_bytes.len() as u64)
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
    encode(input, output, options, keys, None, progress)
}

/// Whether `input` is a cue sheet, by its extension: a CD, whose tracks it
/// names.
fn is_cue(input: &Path) -> bool {
    input
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("cue"))
}

/// Like [`compress`], but the disc's chunks go to the store at `store`, and
/// `output` is a thin ORE naming them there. The store is made with
/// [`create_store`](super::create_store); a game finds it again as the
/// nearest `.ore-store` up from its folder.
pub fn compress_to_store(
    input: &Path,
    output: &Path,
    store: &Path,
    options: &Options,
    keys: &[Key],
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    encode(input, output, options, keys, Some(store), progress)
}

fn encode(
    input: &Path,
    output: &Path,
    options: &Options,
    keys: &[Key],
    store: Option<&Path>,
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    if is_cue(input) {
        return encode_cd(input, output, options, store, progress);
    }
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
        let store = store
            .map(|dir| StoreWriter::open(dir, options.codec, options.profile.group_size()))
            .transpose()?;
        let mut encoder = Encoder::new(&part, options, store)?;
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
                        encoder.flush(false)?;
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
                        encoder.flush(false)?;
                        progress(raw.len() as u64);
                    }
                    segmenter.finish(&mut |run| encoder.push(run));
                }
                AreaKind::CdData | AreaKind::CdAudio => {
                    unreachable!("a disc image's plan has no CD track")
                }
            }
            exception_count += exceptions.len() as u64;
            areas.push(Area {
                len: plan.len,
                file: None,
                kind: plan.kind,
                frame: None,
                start: plan.start,
                records: encoder.records - records,
                sectors: None,
                title_key: plan.key.map(|key| key.to_vec()),
                exceptions,
            });
        }

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
                cue: None,
                sbi: None,
                size,
                areas,
                class: MEDIA_DVD_2048,
                platform: if wii { "wii" } else { "gamecube" }.into(),
            },
            store: None,
            ledger,
            identity: vec![
                identity("blake3-256", logical_hash.to_vec()),
                identity("sha-1", sha1.finalize().to_vec()),
                identity("md5", md5.finalize().to_vec()),
                identity("crc-32", crc32.finalize().to_be_bytes().to_vec()),
            ],
            // Filled in as the ORE is finished
            provenance: Provenance {
                tool: String::new(),
                params: Params {
                    codec: options.codec,
                    group: 0,
                    level: 0,
                    chunker: Chunker {
                        max: 0,
                        min: 0,
                        name: String::new(),
                    },
                },
            },
        };
        Ok(Stats {
            input_size: size,
            output_size: encoder.finish(meta, options, logical_hash)?,
        })
    })();
    io::finish(&part, output, result)
}

/// Move the ORE at `input`'s chunks into the store at `store`, writing a
/// thin ORE naming them there to `output`. Its disc, hashes and metadata
/// stay as they are; chunks the store lacks are compressed as `options` say.
pub fn attach(
    input: &Path,
    output: &Path,
    store: &Path,
    options: &Options,
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    rewrite(input, output, Some(store), options, progress)
}

/// Write the ORE at `input`, thin or not, to `output` as a standalone ORE
/// holding its chunks itself, compressed as `options` say: what exporting a
/// game from a store takes.
pub fn detach(
    input: &Path,
    output: &Path,
    options: &Options,
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    rewrite(input, output, None, options, progress)
}

/// Write an ORE again record for record, its chunks to `store` or inline.
fn rewrite(
    input: &Path,
    output: &Path,
    store: Option<&Path>,
    options: &Options,
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    let input_size = std::fs::metadata(input)?.len();
    let mut reader = Reader::open(input)?;
    let part = io::part(output);
    let result = (|| {
        let store = store
            .map(|dir| StoreWriter::open(dir, options.codec, options.profile.group_size()))
            .transpose()?;
        let mut encoder = Encoder::new(&part, options, store)?;
        let size = reader.size();
        let mut reported = 0;
        reader.runs(&mut |event, done| {
            match event {
                Event::Area(kind) => encoder.set_codec(if kind == AreaKind::CdAudio {
                    Codec::Flac
                } else {
                    options.codec
                })?,
                Event::Run(run) => {
                    encoder.push(run);
                    encoder.flush(false)?;
                }
            }
            // Scale the progress through the disc to the file
            let consumed = (done as u128 * input_size as u128 / size as u128) as u64;
            progress(consumed - reported);
            reported = consumed;
            Ok(())
        })?;
        let (meta, logical_hash) = (reader.meta.clone(), reader.header.logical_hash);
        Ok(Stats {
            input_size,
            output_size: encoder.finish(meta, options, logical_hash)?,
        })
    })();
    io::finish(&part, output, result)
}

/// A CD's audio chunks: 8 sectors, a FLAC frame.
const AUDIO_CHUNK: usize = 8 * SECTOR;
/// How many sectors of a CD track are read at a time: a whole number of
/// audio chunks.
const CD_WINDOW: usize = 256 * 8;

/// Content-defined chunks of a stream given a piece at a time.
#[derive(Default)]
struct Cutter {
    buf: Vec<u8>,
}

impl Cutter {
    fn feed(&mut self, bytes: &[u8], emit: &mut dyn FnMut(Run)) {
        self.buf.extend_from_slice(bytes);
        let mut at = 0;
        while self.buf.len() - at > cdc::MAX {
            let n = cdc::cut(&self.buf[at..]);
            emit(Run::Chunk(&self.buf[at..at + n]));
            at += n;
        }
        self.buf.drain(..at);
    }

    fn finish(&mut self, emit: &mut dyn FnMut(Run)) {
        let mut at = 0;
        while at < self.buf.len() {
            let n = cdc::cut(&self.buf[at..]);
            emit(Run::Chunk(&self.buf[at..at + n]));
            at += n;
        }
        self.buf.clear();
    }
}

/// A file's name, as a cue sheet names it.
fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// Write the CD whose cue sheet is `input`, its tracks a file each, as an
/// ORE: data tracks' sectors split into what is generated and what is
/// stored, audio tracks' PCM in FLAC. An SBI beside the cue sheet, of the
/// same name, is kept too.
fn encode_cd(
    input: &Path,
    output: &Path,
    options: &Options,
    store: Option<&Path>,
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    let cue_bytes = std::fs::read(input)?;
    let dir = input.parent().unwrap_or(Path::new(""));
    let sheet = cue::parse(&String::from_utf8_lossy(&cue_bytes), dir)?;
    let mut files = HashSet::new();
    let mut size = 0;
    for track in &sheet.tracks {
        if !files.insert(track.file.clone()) {
            return Err(Error::Unsupported(
                "a cue sheet with several tracks in one file".into(),
            ));
        }
        let len = std::fs::metadata(&track.file)?.len();
        if len == 0 || !len.is_multiple_of(SECTOR as u64) {
            return Err(Error::Corrupt(format!(
                "{} is not of whole sectors",
                track.file.display()
            )));
        }
        size += len;
    }
    let sbi_path = input.with_extension("sbi");
    let sbi = match std::fs::read(&sbi_path) {
        Ok(bytes) if bytes.starts_with(b"SBI\0") => Some(bytes),
        Ok(_) => {
            return Err(Error::Corrupt(format!(
                "{} is not an SBI",
                sbi_path.display()
            )));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let part = io::part(output);
    let result = (|| {
        let store = store
            .map(|dir| StoreWriter::open(dir, options.codec, options.profile.group_size()))
            .transpose()?;
        let mut encoder = Encoder::new(&part, options, store)?;
        let mut logical = blake3::Hasher::new();
        let mut identity = Vec::new();
        let mut areas = Vec::new();
        let (mut frame, mut start, mut generated) = (LEAD_IN, 0, 0);
        let mut buf = vec![0; CD_WINDOW * SECTOR];
        for track in &sheet.tracks {
            frame += track.pregap;
            let audio = track.mode == Mode::Audio;
            encoder.set_codec(if audio { Codec::Flac } else { options.codec })?;
            let file = File::open(&track.file)?;
            let len = file.metadata()?.len();
            let records = encoder.records;
            let mut hashes = Hashes::default();
            let mut map = MapWriter::default();
            let mut cutter = Cutter::default();
            let mut plane = Vec::new();
            let mut at = 0;
            while at < len {
                let length = (len - at).min(buf.len() as u64) as usize;
                let buf = &mut buf[..length];
                io::read_exact_at(&file, buf, at)?;
                hashes.update(buf);
                logical.update(buf);
                if audio {
                    for chunk in buf.chunks(AUDIO_CHUNK) {
                        if chunk.iter().all(|&b| b == 0) {
                            encoder.push(Run::Zero(chunk.len() as u64));
                        } else {
                            encoder.push(Run::Chunk(chunk));
                        }
                    }
                } else {
                    plane.clear();
                    let sector_at = (at / SECTOR as u64) as u32;
                    for (i, raw) in buf.as_chunks::<SECTOR>().0.iter().enumerate() {
                        let kind = map.push(raw, frame + sector_at + i as u32);
                        let range = data_range(kind);
                        generated += (SECTOR - range.len()) as u64;
                        plane.extend_from_slice(&raw[range]);
                    }
                    cutter.feed(&plane, &mut |run| encoder.push(run));
                }
                encoder.flush(false)?;
                progress(length as u64);
                at += length as u64;
            }
            cutter.finish(&mut |run| encoder.push(run));
            let name = file_name(&track.file);
            let Hashes {
                crc32, md5, sha1, ..
            } = hashes;
            identity.extend(file_identity(&name, crc32, md5, sha1));
            areas.push(Area {
                len,
                file: Some(name),
                kind: if audio {
                    AreaKind::CdAudio
                } else {
                    AreaKind::CdData
                },
                frame: Some(frame as u64),
                start,
                records: encoder.records - records,
                sectors: (!audio).then(|| map.finish()),
                title_key: None,
                exceptions: Vec::new(),
            });
            frame += (len / SECTOR as u64) as u32;
            start += len;
        }
        for (name, bytes) in [
            (file_name(input), Some(&cue_bytes)),
            (file_name(&sbi_path), sbi.as_ref()),
        ] {
            if let Some(bytes) = bytes {
                let mut hashes = Hashes::default();
                hashes.update(bytes);
                let Hashes {
                    crc32, md5, sha1, ..
                } = hashes;
                identity.extend(file_identity(&name, crc32, md5, sha1));
            }
        }
        let logical_hash: [u8; 32] = logical.finalize().into();
        identity.insert(
            0,
            Identity {
                algo: "blake3-256".into(),
                view: "logical".into(),
                value: logical_hash.to_vec(),
            },
        );
        let audio = areas.iter().any(|area| area.kind == AreaKind::CdAudio);
        let meta = Meta {
            media: Media {
                cue: Some(cue_bytes.clone()),
                sbi: sbi.clone(),
                size,
                areas,
                class: if audio { MEDIA_CD_MIXED } else { MEDIA_CD_2352 },
                platform: "cd".into(),
            },
            store: None,
            ledger: vec![
                step("cd-sectors", Some(generated)),
                step("zero", Some(encoder.zeros)),
                step("mincdc-dedup", Some(encoder.deduped)),
            ],
            identity,
            // Filled in as the ORE is finished
            provenance: Provenance {
                tool: String::new(),
                params: Params {
                    codec: options.codec,
                    group: 0,
                    level: 0,
                    chunker: Chunker {
                        max: 0,
                        min: 0,
                        name: String::new(),
                    },
                },
            },
        };
        Ok(Stats {
            input_size: size,
            output_size: encoder.finish(meta, options, logical_hash)?,
        })
    })();
    io::finish(&part, output, result)
}

/// A file's CRC-32, MD5 and SHA-1, as a DAT lists them, by its name.
fn file_identity(name: &str, crc32: crc32fast::Hasher, md5: Md5, sha1: Sha1) -> [Identity; 3] {
    let identity = |algo: &str, value: Vec<u8>| Identity {
        algo: algo.into(),
        view: name.into(),
        value,
    };
    [
        identity("sha-1", sha1.finalize().to_vec()),
        identity("md5", md5.finalize().to_vec()),
        identity("crc-32", crc32.finalize().to_be_bytes().to_vec()),
    ]
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

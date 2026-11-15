//! Writing an ORE from a disc image.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use md5::Md5;
use rayon::prelude::*;
use sha1::{Digest, Sha1};

use super::cd::cue::{self, Mode};
use super::cd::iso9660;
use super::cd::sector::{LEAD_IN, SECTOR, data_range};
use super::cdc;
use super::cdtrack::{MapWriter, Plane};
use super::codec::{FLAC_LEVEL, LZMA2_PRESET, ZSTD_LEVEL};
use super::crypto::Key;
use super::disc::{Iso, PARTITION_DATA_SIZE, Platform, Source, Volume};
use super::error::{Error, Result};
use super::format::*;
use super::fst::FileSystem;
use super::io;
use super::pack::Packer;
use super::ps2audio;
use super::reader::{Event, Reader};
use super::segment::{BLOCK, Run, Segmenter};
use super::store::StoreWriter;
use super::video;
use super::wii::{
    self, BLOCK_DATA_SIZE, BLOCK_TOTAL_SIZE, BLOCKS_PER_GROUP, BlockData, GROUP_DATA_SIZE,
    GROUP_TOTAL_SIZE, HashBlock,
};
use super::xa;

/// How an ORE is written.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    pub profile: Profile,
    pub codec: Codec,
    /// The cartridge the input is the ROM of, one of [`CARTRIDGE_PLATFORMS`],
    /// when the caller knows it, as a ROM manager does from its DAT: the ROM
    /// is then taken whole, its header not checked, which a console such as
    /// the NES or the PC Engine has none of. `None` recognises a disc or a
    /// cartridge by its header, refusing what it does not know.
    pub platform: Option<&'static str>,
}

/// The cartridges a ROM can be named the ROM of, in [`Options::platform`].
pub const CARTRIDGE_PLATFORMS: [&str; 30] = [
    "32x",
    "a2600",
    "a5200",
    "a7800",
    "colecovision",
    "dsi",
    "gb",
    "gba",
    "gbc",
    "gg",
    "jaguar",
    "lynx",
    "md",
    "n64",
    "nds",
    "nes",
    "ngp",
    "ngpc",
    "odyssey2",
    "pce",
    "pokemini",
    "satellaview",
    "sg1000",
    "sgx",
    "sms",
    "snes",
    "sufami",
    "vb",
    "ws",
    "wsc",
];

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
    /// Its chunks' codec, when not the ORE's: a PlayStation 2 DVD's audio,
    /// cut in fixed chunks.
    codec: Option<Codec>,
}

/// A PlayStation 2 DVD's audio chunks: a whole number of SPU blocks and of
/// PCM periods, from where the stream's area starts.
const AUDIO_FIXED: usize = 16 << 10;

/// A PlayStation 2 DVD or a PSP UMD read once, in order, for its hashes and its audio:
/// its areas, the rest in disc order, then the audio, each codec's
/// together.
fn plan_sony(iso: &mut Iso, hashes: &mut Hashes) -> Result<Vec<Plan>> {
    let size = iso.size();
    let mut scanner = ps2audio::Scanner::new(size);
    let mut buf = vec![0; WINDOW as usize + ps2audio::LOOKAHEAD];
    let mut at = 0;
    while at < size {
        let length = (size - at).min(buf.len() as u64) as usize;
        iso.read_at(at, &mut buf[..length])?;
        let scan = (size - at).min(WINDOW) as usize;
        rayon::join(
            || hashes.update(&buf[..scan]),
            || scanner.feed(&buf[..length], at, scan),
        );
        at += WINDOW;
    }
    let audio = scanner.finish();
    let mut plans = Vec::new();
    let mut at = 0;
    for stream in &audio {
        if stream.start > at {
            plans.push(raw(at, stream.start - at));
        }
        at = stream.start + stream.len;
    }
    if size > at || plans.is_empty() && audio.is_empty() {
        plans.push(raw(at, size - at));
    }
    let mut audio = audio;
    audio.sort_by_key(|stream| (stream.codec.id(), stream.start));
    plans.extend(audio.into_iter().map(|stream| Plan {
        codec: Some(stream.codec),
        ..raw(stream.start, stream.len)
    }));
    Ok(plans)
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
            codec: None,
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
                codec: None,
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
        codec: None,
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
enum Sink<'a> {
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
        writer: &'a mut StoreWriter,
        last: u64,
        used: HashSet<u64>,
    },
}

/// An ORE being written: its index records as runs come, its stored chunks
/// to its sink.
struct Encoder<'a> {
    file: BufWriter<File>,
    index: Vec<u8>,
    records: u64,
    record_max: u64,
    sink: Sink<'a>,
    padding: u64,
    zeros: u64,
    /// Bytes stored as runs of one byte not zero.
    filled: u64,
    deduped: u64,
}

impl<'a> Encoder<'a> {
    fn new(part: &Path, options: &Options, store: Option<&'a mut StoreWriter>) -> Result<Self> {
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
            filled: 0,
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
            Run::Fill(n, byte) => {
                self.index.push(KIND_GENERATED);
                put_uvarint(&mut self.index, n);
                self.index.push(GEN_FILL);
                self.index.push(byte);
                self.filled += n;
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
        // A store's last group stays open for the next ORE of a batch: its
        // commit writes it
        self.flush(matches!(self.sink, Sink::Inline { .. }))?;
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
                Codec::Xa | Codec::Str | Codec::Vag => 0,
                Codec::Pcm => FLAC_LEVEL,
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
        // The store is committed, by the batch, once the ORE naming its
        // chunks is whole
        if let Sink::Store { writer, used, .. } = self.sink {
            for id in used {
                writer.reference(id);
            }
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
    let part = io::part(output);
    let result = encode(input, &part, options, keys, None, progress);
    io::finish(&part, output, result)
}

/// The codec `input` is best written with when nothing says otherwise:
/// LZMA2 for a cartridge's ROM, small and read whole, which Zstandard
/// loses to 7z on by 7%; Zstandard for the rest, the quicker to read.
/// `platform` is [`Options::platform`]: a ROM named a cartridge's is one.
pub fn default_codec(input: &Path, platform: Option<&str>) -> Result<Codec> {
    if platform.is_some() {
        return Ok(Codec::Lzma2);
    }
    if is_cue(input) {
        return Ok(Codec::Zstd);
    }
    Ok(match cartridge_platform(&mut Iso::open(input)?)? {
        Some(_) => Codec::Lzma2,
        None => Codec::Zstd,
    })
}

/// The Nintendo logo a Game Boy's boot ROM checks a cartridge for.
const GB_LOGO: [u8; 48] = [
    0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00, 0x0D,
    0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD, 0xD9, 0x99,
    0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
];

/// The largest cartridge ROM taken, past the DS's 512 MiB.
const CARTRIDGE_MAX: u64 = 1 << 30;

/// The CRC-16 a DS checks its header with: polynomial 0xA001, reflected,
/// from 0xFFFF.
fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0xFFFFu16;
    for &byte in bytes {
        crc ^= byte as u16;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                crc >> 1 ^ 0xA001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

/// Which cartridge `iso` is the ROM of, if one, by what its console checks
/// in its header:
/// - a DS's logo CRC, 0xCF56, and its header's CRC-16;
/// - an N64's first word, in any of the three byte orders dumps come in;
/// - a Game Boy's Nintendo logo, `gbc` when it flags Game Boy Color
///   support;
/// - a Game Boy Advance's fixed byte and header checksum;
/// - a Mega Drive's `SEGA` at 0x100, which a 32X's has too.
fn cartridge_platform(iso: &mut Iso) -> Result<Option<&'static str>> {
    let size = iso.size();
    if !(0x150..=CARTRIDGE_MAX).contains(&size) {
        return Ok(None);
    }
    let mut header = [0; 0x200];
    iso.read_at(0, &mut header[..(size as usize).min(0x200)])?;
    let le16 = |at: usize| u16::from_le_bytes([header[at], header[at + 1]]);
    if size >= 0x200 && le16(0x15C) == 0xCF56 && le16(0x15E) == crc16(&header[..0x15E]) {
        return Ok(Some("nds"));
    }
    if matches!(
        header[..4],
        [0x80, 0x37, 0x12, 0x40] | [0x37, 0x80, 0x40, 0x12] | [0x40, 0x12, 0x37, 0x80]
    ) {
        return Ok(Some("n64"));
    }
    if header[0x104..0x134] == GB_LOGO {
        return Ok(Some(if header[0x143] & 0x80 != 0 {
            "gbc"
        } else {
            "gb"
        }));
    }
    let checksum = header[0xA0..0xBD]
        .iter()
        .fold(0u8, |sum, &b| sum.wrapping_sub(b))
        .wrapping_sub(0x19);
    if header[0xB2] == 0x96 && header[0xBD] == checksum {
        return Ok(Some("gba"));
    }
    Ok((header[0x100..0x104] == *b"SEGA" || header[0x101..0x105] == *b"SEGA").then_some("md"))
}

/// Which of Sony's discs `iso` is, if one: a PlayStation 2 DVD, an ISO
/// 9660 file system whose `SYSTEM.CNF` boots a `BOOT2` executable, or a
/// PSP UMD, one holding `UMD_DATA.BIN` or `PSP_GAME/PARAM.SFO`.
fn sony_platform(iso: &mut Iso) -> Result<Option<&'static str>> {
    let mut read = |lba: u32, sector: &mut [u8; 2048]| {
        iso.read_at(u64::from(lba) * 2048, sector)
            .map_err(|error| std::io::Error::other(error.to_string()))
    };
    let Ok(entries) = iso9660::entries(&mut read) else {
        return Ok(None);
    };
    let file = |path: &str| {
        entries
            .iter()
            .find(|entry| !entry.directory && entry.path.eq_ignore_ascii_case(path))
    };
    if file("/UMD_DATA.BIN")
        .or(file("/PSP_GAME/PARAM.SFO"))
        .is_some()
    {
        return Ok(Some("psp"));
    }
    let Some(cnf) = file("/SYSTEM.CNF") else {
        return Ok(None);
    };
    let mut bytes = vec![0; (cnf.size as usize).min(4096)];
    iso.read_at(u64::from(cnf.lba) * 2048, &mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes)
        .contains("BOOT2")
        .then_some("ps2"))
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
    let mut batch = StoreBatch::open(store, options)?;
    let stats = batch.compress(input, output, options, keys, progress)?;
    batch.commit()?;
    Ok(stats)
}

/// Games written to one store at once, as one write: their chunks share
/// its groups and segments, where a write a game would give each its own,
/// sealed, and a library of small ROMs would pay a segment's parity rows a
/// ROM. The store holds them, and each thin ORE lands at its output, once
/// [`StoreBatch::commit`] has run; a batch dropped before leaves the store
/// as it was and writes none.
pub struct StoreBatch {
    writer: StoreWriter,
    /// Each thin ORE written, and where it goes on commit.
    parts: Vec<(PathBuf, PathBuf)>,
}

impl StoreBatch {
    /// Open the store at `store`, waiting for any other writer to be done:
    /// one writer at a time, for as long as the batch lives. Its groups are
    /// of `options`' size; each game's chunks are compressed as its own
    /// options say.
    pub fn open(store: &Path, options: &Options) -> Result<Self> {
        Ok(Self {
            writer: StoreWriter::open(store, options.codec, options.profile.group_size())?,
            parts: Vec::new(),
        })
    }

    /// [`compress_to_store`] in the batch.
    pub fn compress(
        &mut self,
        input: &Path,
        output: &Path,
        options: &Options,
        keys: &[Key],
        progress: &mut dyn FnMut(u64),
    ) -> Result<Stats> {
        self.add(output, |part, writer| {
            encode(input, part, options, keys, Some(writer), progress)
        })
    }

    /// [`attach`] in the batch.
    pub fn attach(
        &mut self,
        input: &Path,
        output: &Path,
        options: &Options,
        progress: &mut dyn FnMut(u64),
    ) -> Result<Stats> {
        self.add(output, |part, writer| {
            rewrite(input, part, Some(writer), options, progress)
        })
    }

    /// Write a thin ORE beside `output`, to land there on commit. A game
    /// that fails leaves its chunks in the store unused, as a compaction finds
    /// them.
    fn add(
        &mut self,
        output: &Path,
        write: impl FnOnce(&Path, &mut StoreWriter) -> Result<Stats>,
    ) -> Result<Stats> {
        let part = io::part(output);
        match write(&part, &mut self.writer) {
            Ok(stats) => {
                self.parts.push((part, output.to_path_buf()));
                Ok(stats)
            }
            Err(e) => {
                let _ = std::fs::remove_file(&part);
                Err(e)
            }
        }
    }

    /// Commit the store, then put each thin ORE in place. Returns the bytes
    /// the store's segments took.
    pub fn commit(mut self) -> Result<u64> {
        let added = self.writer.commit()?;
        for (part, output) in std::mem::take(&mut self.parts) {
            std::fs::rename(&part, &output)?;
        }
        Ok(added)
    }
}

impl Drop for StoreBatch {
    fn drop(&mut self) {
        for (part, _) in &self.parts {
            let _ = std::fs::remove_file(part);
        }
    }
}

fn encode(
    input: &Path,
    part: &Path,
    options: &Options,
    keys: &[Key],
    store: Option<&mut StoreWriter>,
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    if is_cue(input) {
        return encode_cd(input, part, options, store, progress);
    }
    let mut iso = Iso::open(input)?;
    let size = iso.size();
    // A GameCube or Wii disc is cut by its structure; a PlayStation 2 DVD
    // and a cartridge are one area, their zeros and the rest
    let mut hashes = Hashes::default();
    let cartridge = match options.platform {
        Some(platform) if !CARTRIDGE_PLATFORMS.contains(&platform) => {
            return Err(Error::Unsupported(format!("no cartridge named {platform}")));
        }
        Some(_) if !(1..=CARTRIDGE_MAX).contains(&size) => {
            return Err(Error::Unsupported(format!(
                "a ROM of {size} bytes, past 1 GiB or empty"
            )));
        }
        Some(platform) => Some(platform),
        None => cartridge_platform(&mut iso)?,
    };
    let (plans, platform) = match cartridge.map(|platform| (vec![raw(0, size)], platform)) {
        Some(planned) => planned,
        None => match Volume::new(&mut iso, keys)? {
            Some(mut volume) => {
                let platform = match volume.platform {
                    Platform::GameCube => "gamecube",
                    Platform::Wii => "wii",
                };
                (plan(&mut iso, &mut volume)?, platform)
            }
            None => match sony_platform(&mut iso)? {
                Some(platform) => (plan_sony(&mut iso, &mut hashes)?, platform),
                None => {
                    return Err(Error::Unsupported(
                    "neither a GameCube, a Wii, a PlayStation 2 nor a PSP disc, nor a cartridge ROM recognised by its header: name its platform".into(),
                ));
                }
            },
        },
    };
    // A Sony disc is hashed as it is planned; only Nintendo's discs have
    // padding generated
    let sony = matches!(platform, "ps2" | "psp");
    let padded = matches!(platform, "gamecube" | "wii");
    (|| {
        let mut encoder = Encoder::new(part, options, store)?;
        let mut areas = Vec::new();
        let mut exception_count = 0;
        for plan in &plans {
            let records = encoder.records;
            let fs = plan.file_system.as_deref();
            let mut exceptions = Vec::new();
            encoder.set_codec(plan.codec.unwrap_or(options.codec))?;
            match plan.kind {
                AreaKind::Raw if plan.codec.is_some() => {
                    let mut buf = vec![0; WINDOW as usize];
                    let mut at = 0;
                    while at < plan.len {
                        let length = (plan.len - at).min(WINDOW) as usize;
                        let buf = &mut buf[..length];
                        iso.read_at(plan.start + at, buf)?;
                        for chunk in buf.chunks(AUDIO_FIXED) {
                            if chunk.iter().all(|&b| b == 0) {
                                encoder.push(Run::Zero(chunk.len() as u64));
                            } else {
                                encoder.push(Run::Chunk(chunk));
                            }
                        }
                        encoder.flush(false)?;
                        progress(length as u64);
                        at += length as u64;
                    }
                }
                AreaKind::Raw => {
                    let mut segmenter = if padded {
                        Segmenter::new(plan.start)
                    } else {
                        Segmenter::without_padding(plan.start)
                    };
                    let mut buf = vec![0; (WINDOW + BLOCK) as usize];
                    let mut at = 0;
                    while at < plan.len {
                        // Windows end on blocks, as the generator counts them
                        let end = (plan.start + at + WINDOW).next_multiple_of(BLOCK) - plan.start;
                        let length = (end.min(plan.len) - at) as usize;
                        let buf = &mut buf[..length];
                        iso.read_at(plan.start + at, buf)?;
                        if !sony {
                            hashes.update(buf);
                        }
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
                // A ROM comes back under its name
                file: cartridge.map(|_| file_name(input)),
                kind: plan.kind,
                frame: None,
                start: plan.start,
                records: encoder.records - records,
                sectors: None,
                title_key: plan.key.map(|key| key.to_vec()),
                codec: plan.codec,
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
        let mut ledger = Vec::new();
        if areas.iter().any(|area| area.kind == AreaKind::WiiPartition) {
            ledger.push(step("wii-decrypt", None));
            ledger.push(step("wii-hashes", Some(exception_count)));
        }
        if padded {
            ledger.push(step("gc-wii-filler-prng", Some(encoder.padding)));
        }
        ledger.push(step("zero", Some(encoder.zeros)));
        if encoder.filled > 0 {
            ledger.push(step("fill", Some(encoder.filled)));
        }
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
                class: if cartridge.is_some() {
                    MEDIA_ROM_CARTRIDGE
                } else {
                    MEDIA_DVD_2048
                },
                platform: platform.into(),
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
    })()
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
    let mut batch = StoreBatch::open(store, options)?;
    let stats = batch.attach(input, output, options, progress)?;
    batch.commit()?;
    Ok(stats)
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
    let part = io::part(output);
    let result = rewrite(input, &part, None, options, progress);
    io::finish(&part, output, result)
}

/// Write the CD ORE at `input` to `output` with the cue sheet at `cue` in
/// place of its own, its metadata alone written again: its tracks, their
/// hashes and its chunks, inline or in a store, stay as they are. The cue
/// sheet's name and its tracks' are `cue`'s, and an SBI it holds follows the
/// cue sheet's name.
///
/// `cue` must describe the same tracks, audio and data as they were, at the
/// same frames, so its `PREGAP`s too; its `INDEX`es, data tracks' modes and
/// anything else it says may differ. `progress` is called with the input
/// bytes consumed since the last call.
pub fn replace_cue(
    input: &Path,
    output: &Path,
    cue: &Path,
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    let file = File::open(input)?;
    let input_size = file.metadata()?.len();
    let mut bytes = [0; HEADER_LEN];
    io::read_exact_at(&file, &mut bytes, 0)?;
    let mut header = Header::read(&bytes)?;
    // The metadata is written last, so all before it is kept as it is
    if u64::from(header.meta_len) > META_MAX
        || header.meta_offset.checked_add(u64::from(header.meta_len)) != Some(input_size)
    {
        return Err(Error::Corrupt("metadata not at the end of the file".into()));
    }
    let mut meta_bytes = vec![0; header.meta_len as usize];
    io::read_exact_at(&file, &mut meta_bytes, header.meta_offset)?;
    if crc32c::crc32c(&meta_bytes) != header.meta_crc {
        return Err(Error::Corrupt("metadata checksum".into()));
    }
    let mut meta: Meta = from_cbor(&meta_bytes)?;
    if meta.media.cue.is_none() {
        return Err(Error::Unsupported("a cue sheet for a disc image".into()));
    }

    let cue_bytes = std::fs::read(cue)?;
    let sheet = cue::parse(&String::from_utf8_lossy(&cue_bytes), Path::new(""))?;
    if sheet.tracks.len() != meta.media.areas.len() {
        return Err(Error::Unsupported(format!(
            "a cue sheet of {} tracks for {}",
            sheet.tracks.len(),
            meta.media.areas.len()
        )));
    }
    let cue_name = file_name(cue);
    let sbi_name = file_name(&cue.with_extension("sbi"));
    let mut renames = Vec::new();
    let starts = cue::starts(
        &sheet,
        LEAD_IN,
        meta.media.areas.iter().map(|area| area.len / SECTOR as u64),
    );
    for ((track, area), frame) in sheet.tracks.iter().zip(&mut meta.media.areas).zip(starts) {
        let audio = track.mode == Mode::Audio;
        if audio != (area.kind == AreaKind::CdAudio) || area.frame != Some(frame) {
            return Err(Error::Unsupported(format!(
                "a cue sheet whose track {} is not the ORE's",
                track.number
            )));
        }
        // A track's file lies beside the cue sheet, a file a track
        let name = file_name(&track.file);
        if track.file.as_os_str() != name.as_str()
            || name == cue_name
            || name == sbi_name
            || renames.iter().any(|(_, other)| *other == name)
        {
            return Err(Error::Corrupt(format!("a track's file name: {name:?}")));
        }
        renames.push((area.file.replace(name.clone()).unwrap_or_default(), name));
    }

    // The tracks' hashes under their new names, the old cue sheet's for the
    // new one's, before the SBI's
    let mut identity = Vec::new();
    let mut sbi = Vec::new();
    for mut entry in std::mem::take(&mut meta.identity) {
        if entry.view == "logical" {
            identity.push(entry);
        } else if let Some((_, name)) = renames.iter().find(|(old, _)| *old == entry.view) {
            entry.view = name.clone();
            identity.push(entry);
        } else if entry.view.to_lowercase().ends_with(".sbi") {
            entry.view = sbi_name.clone();
            sbi.push(entry);
        }
    }
    let mut hashes = Hashes::default();
    hashes.update(&cue_bytes);
    let Hashes {
        crc32, md5, sha1, ..
    } = hashes;
    identity.extend(file_identity(&cue_name, crc32, md5, sha1));
    identity.extend(sbi);
    meta.identity = identity;
    meta.media.cue = Some(cue_bytes);

    let meta_bytes = to_cbor(&meta);
    header.meta_len = u32::try_from(meta_bytes.len())
        .ok()
        .filter(|&len| u64::from(len) <= META_MAX)
        .ok_or_else(|| Error::Unsupported("metadata past its bound".into()))?;
    header.meta_crc = crc32c::crc32c(&meta_bytes);
    let part = io::part(output);
    let result = (|| {
        let mut out = File::create(&part)?;
        std::io::copy(
            &mut std::io::Read::take(&file, header.meta_offset),
            &mut out,
        )?;
        out.write_all(&meta_bytes)?;
        out.seek(SeekFrom::Start(0))?;
        out.write_all(&header.write())?;
        out.sync_all()?;
        progress(input_size);
        Ok(Stats {
            input_size,
            output_size: header.meta_offset + meta_bytes.len() as u64,
        })
    })();
    io::finish(&part, output, result)
}

/// Write an ORE again record for record, its chunks to `store` or inline.
fn rewrite(
    input: &Path,
    part: &Path,
    store: Option<&mut StoreWriter>,
    options: &Options,
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    let input_size = std::fs::metadata(input)?.len();
    let mut reader = Reader::open(input)?;
    (|| {
        let mut encoder = Encoder::new(part, options, store)?;
        let size = reader.size();
        let mut reported = 0;
        reader.runs(&mut |event, done| {
            match event {
                Event::Area(kind, codec) => encoder.set_codec(match codec {
                    Some(codec) => codec,
                    None if kind == AreaKind::CdAudio => Codec::Flac,
                    None => options.codec,
                })?,
                Event::Video => encoder.set_codec(Codec::Str)?,
                Event::Xa => encoder.set_codec(Codec::Xa)?,
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
    })()
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
    part: &Path,
    options: &Options,
    store: Option<&mut StoreWriter>,
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
    (|| {
        let mut encoder = Encoder::new(part, options, store)?;
        let mut logical = blake3::Hasher::new();
        let mut identity = Vec::new();
        let mut areas = Vec::new();
        let (mut start, mut generated) = (0, 0);
        let mut buf = vec![0; CD_WINDOW * SECTOR];
        let lens = sheet
            .tracks
            .iter()
            .map(|track| Ok(std::fs::metadata(&track.file)?.len() / SECTOR as u64))
            .collect::<Result<Vec<_>>>()?;
        let starts = cue::starts(&sheet, LEAD_IN, lens);
        for (track, &frame) in sheet.tracks.iter().zip(&starts) {
            let frame = frame as u32;
            let audio = track.mode == Mode::Audio;
            encoder.set_codec(if audio { Codec::Flac } else { options.codec })?;
            let file = File::open(&track.file)?;
            let len = file.metadata()?.len();
            let records = encoder.records;
            let mut hashes = Hashes::default();
            let mut map = MapWriter::default();
            let mut cutter = Cutter::default();
            let mut plane = Vec::new();
            // The video and XA sectors, by number, stored after the rest
            let mut sectors = [Vec::new(), Vec::new(), Vec::new()];
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
                        let (kind, at) = map.push(raw, frame + sector_at + i as u32);
                        let range = data_range(kind);
                        generated += (SECTOR - range.len()) as u64;
                        if at == Plane::Data {
                            plane.extend_from_slice(&raw[range]);
                        } else {
                            sectors[at as usize].push(sector_at + i as u32);
                        }
                    }
                    cutter.feed(&plane, &mut |run| encoder.push(run));
                }
                encoder.flush(false)?;
                progress(length as u64);
                at += length as u64;
            }
            cutter.finish(&mut |run| encoder.push(run));
            for (plane, codec) in [(Plane::Video, Codec::Str), (Plane::Xa, Codec::Xa)] {
                if !sectors[plane as usize].is_empty() {
                    encoder.set_codec(codec)?;
                    push_plane(
                        &mut encoder,
                        &file,
                        plane,
                        &sectors[plane as usize],
                        &mut buf,
                    )?;
                }
            }
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
                codec: None,
                exceptions: Vec::new(),
            });
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
        let gd_rom = sheet.tracks.iter().any(|track| track.high_density);
        let meta = Meta {
            media: Media {
                cue: Some(cue_bytes.clone()),
                sbi: sbi.clone(),
                size,
                areas,
                class: match (gd_rom, audio) {
                    (true, _) => MEDIA_GD_ROM,
                    (false, true) => MEDIA_CD_MIXED,
                    (false, false) => MEDIA_CD_2352,
                },
                platform: if gd_rom { "gd-rom" } else { "cd" }.into(),
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
    })()
}

/// The most XA records in a chunk, 37 KB.
const XA_CHUNK: usize = 16;
/// The most video sectors in a chunk, 128 KB, more than a frame.
const VIDEO_CHUNK: usize = 64;

/// The video or XA plane of a data track, read again from `file`: the
/// sectors `sectors`, each a record, cut into chunks so that a run of
/// sectors another disc holds is the same chunks there. A video record is
/// a sector's data, cut where the frame changes; an XA record is a
/// sector's subheader and data, cut after any whose CRC-32C is a multiple
/// of 8.
fn push_plane(
    encoder: &mut Encoder,
    file: &File,
    plane: Plane,
    sectors: &[u32],
    buf: &mut [u8],
) -> Result<()> {
    let window = (buf.len() / SECTOR) as u32;
    let most = match plane {
        Plane::Video => VIDEO_CHUNK,
        _ => XA_CHUNK,
    };
    let mut chunk = Vec::with_capacity(most * xa::RECORD);
    let (mut records, mut frame) = (0, None);
    for run in sectors.chunk_by(|a, b| a / window == b / window) {
        let first = run[0] / window * window;
        let length = (run.last().unwrap() + 1 - first) as usize * SECTOR;
        io::read_exact_at(file, &mut buf[..length], first as u64 * SECTOR as u64)?;
        for &i in run {
            let raw = &buf[(i - first) as usize * SECTOR..][..SECTOR];
            if plane == Plane::Video {
                let record = &raw[24..24 + video::record_len(&raw[24..])];
                let this = Some(video::frame_of(record));
                if !chunk.is_empty() && this != frame {
                    encoder.push(Run::Chunk(&chunk));
                    (chunk, records) = (Vec::new(), 0);
                }
                frame = this;
                chunk.extend_from_slice(record);
            } else {
                let record = [&raw[16..20], &raw[24..24 + xa::RECORD - 4]].concat();
                chunk.extend_from_slice(&record);
                if crc32c::crc32c(&record).is_multiple_of(8) {
                    records = most - 1;
                }
            }
            records += 1;
            if records == most {
                encoder.push(Run::Chunk(&chunk));
                (chunk, records) = (Vec::new(), 0);
            }
        }
        encoder.flush(false)?;
    }
    if !chunk.is_empty() {
        encoder.push(Run::Chunk(&chunk));
    }
    Ok(())
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

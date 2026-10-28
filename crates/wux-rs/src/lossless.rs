//! What a WUA keeps, besides its titles and their revert data, to rebuild the
//! disc it was made from byte for byte: under `.wud/`, which Cemu ignores.
//!
//! Every sector of a Wii U disc is one of: the disc header, before the first
//! partition; a partition's header, before its FST; one of its regions, the
//! FST and the clusters or contents it lists; or a repeat of the region
//! before it, period the region's length, up to the next region or the
//! partition's end, as the disc's mastering fills free space.
//!
//! So a disc is rebuilt from its headers, its regions and that rule:
//!
//! - the game partition's contents from the WUA's game, rebuilt from its
//!   revert data;
//! - every other partition's regions from a stream of their bytes, decrypted
//!   if the disc key encrypts them, so the same update partition, encrypted
//!   with each disc's key, is the same stream on every disc. Inside it, each
//!   title's `.app`s are decrypted with their title key too, as that is what
//!   lets them compress. An update partition's stream goes to a store
//!   shared by every disc, under its SHA-1, when one is given: a zstd
//!   stream, as it is written once and read through, compressed with a
//!   window as large as its titles' repeats need.
//!
//! The rule is checked on every sector as the disc is read, and a sector it
//! gets wrong is kept whole, so the rebuild is exact either way.
//!
//! `layout` is all big-endian: see [`Layout::to_bytes`].

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::rc::Rc;

use md5::Md5;
use sha1::{Digest, Sha1};

use crate::crypto::{Key, cbc_decrypt, decrypt_block};
use crate::disc::{self, Fst, Mode, PartitionSource, SECTOR};
use crate::error::{Error, Result, corrupt};
use crate::image::Image;
use crate::io::{Scaled, finish};
use crate::nus;
use crate::revert::encrypt;
use crate::wua::{self, DiscGame, parse_ticket, ticket_iv, tree};
use crate::zarchive::{self, Writer};

/// The root folder of a WUA holding what rebuilds its disc.
pub const DISC_DIR: &str = ".wud";
/// The folder update partitions are kept in, shared by the WUAs next to it.
pub const STORE_DIR: &str = ".update-partitions";

const LAYOUT_MAGIC: &[u8; 4] = b"WUDL";
const LAYOUT_VERSION: u32 = 1;
const ENTRY_MAGIC: &[u8; 4] = b"WUUP";
const ENTRY_VERSION: u32 = 1;
/// A store entry's zstd level and window: what keeps an update partition
/// smallest without taking much longer.
const ENTRY_LEVEL: i32 = 19;
pub(crate) const ENTRY_WINDOW_LOG: u32 = 27;
/// A 64 KiB block, of a hashed or blocks cluster, or of a hashed content.
const BLOCK: u64 = 0x10000;
const HASHES: usize = 0x400;
/// How much is read, decrypted or written at a time.
const CHUNK: u64 = 16 * BLOCK;

/// The disc a lossless WUA rebuilds: its size and hashes, those a DAT lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscInfo {
    pub size: u64,
    pub crc32: u32,
    pub md5: [u8; 16],
    pub sha1: [u8; 20],
}

/// Where a region's bytes come from, and how they are encrypted on the disc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// The game's content of this index, from the WUA.
    Content(u16),
    /// The stream, as it is on the disc.
    Plain,
    /// The stream, one CBC stream under the disc key, its IV the index.
    Raw(u16),
    /// The stream, in 64 KiB CBC blocks under the disc key.
    Blocks(u16),
}

/// A region: its first sector in its partition, and its length in bytes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Region {
    pub start: u64,
    pub len: u64,
    pub kind: Kind,
}

impl Region {
    pub fn sectors(&self) -> u64 {
        self.len.div_ceil(SECTOR)
    }
}

/// Where a partition's stream is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Storage {
    /// It has none: the game partition.
    None,
    /// In the WUA.
    Embedded,
    /// In the store, under its SHA-1.
    Store,
}

pub(crate) struct PartitionLayout {
    pub name: String,
    /// Its first sector on the disc, and the next partition's, or the end.
    pub start: u64,
    pub end: u64,
    pub header_sectors: u64,
    pub storage: Storage,
    /// The SHA-1 and length of its stream, decrypted, but not with title keys.
    pub stream_sha1: [u8; 20],
    pub stream_len: u64,
    pub regions: Vec<Region>,
}

pub(crate) struct Layout {
    pub info: DiscInfo,
    /// The WUA folder of the game whose contents fill the game partition.
    pub game: String,
    /// The sectors before the first partition.
    pub header_sectors: u64,
    pub partitions: Vec<PartitionLayout>,
    /// The sectors kept whole, in the order `patches` holds them.
    pub patches: Vec<u64>,
}

/// Tickets, and their title keys, by where they are in a stream.
pub(crate) type Tickets = HashMap<(u64, u32), Vec<u8>>;
pub(crate) type TitleKeys = HashMap<(u64, u32), Key>;
/// Sectors kept whole, by index.
type Patches = Vec<(u64, Vec<u8>)>;

/// A title's content, in a stream decrypted with its title key: where it
/// is, what it is, where its ticket is, which holds that key, and the SHA-1
/// of the common key that key is encrypted with: the Wii U's, or for a vWii
/// title, the vWii's, which a rebuild finds among its keys by that SHA-1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Transform {
    pub offset: u64,
    pub len: u64,
    pub hashed: bool,
    pub index: u16,
    pub ticket: (u64, u32),
    pub common: [u8; 20],
}

/// What makes up a partition, sector by sector: its header, its regions,
/// and runs repeating the region before them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Header,
    Region(usize),
    /// `count` sectors, each the one `period` sectors before it; `None` if
    /// nothing comes before, which only patches can fill.
    Fill {
        count: u64,
        period: Option<u64>,
    },
}

impl PartitionLayout {
    /// The partition's steps, covering it whole.
    pub fn steps(&self) -> Result<Vec<Step>> {
        let size = self.end - self.start;
        let mut steps = Vec::new();
        let mut cursor = self.header_sectors;
        let mut period = None;
        if self.header_sectors > 0 {
            steps.push(Step::Header);
            period = Some(self.header_sectors);
        }
        for (i, region) in self.regions.iter().enumerate() {
            if region.start < cursor || region.start + region.sectors() > size {
                return Err(corrupt(format!(
                    "partition {}'s regions overlap or overflow it",
                    self.name
                )));
            }
            if region.start > cursor {
                steps.push(Step::Fill {
                    count: region.start - cursor,
                    period,
                });
            }
            steps.push(Step::Region(i));
            cursor = region.start + region.sectors();
            if region.sectors() > 0 {
                period = Some(region.sectors());
            }
        }
        if size > cursor {
            steps.push(Step::Fill {
                count: size - cursor,
                period,
            });
        }
        Ok(steps)
    }

    /// Each stream region's offset in the stream, by region index.
    pub fn stream_offsets(&self) -> Vec<Option<u64>> {
        let mut at = 0;
        self.regions
            .iter()
            .map(|region| match region.kind {
                Kind::Content(_) => None,
                _ => {
                    let offset = at;
                    at += region.len;
                    Some(offset)
                }
            })
            .collect()
    }
}

/// Big-endian bytes, appended.
#[derive(Default)]
struct Out(Vec<u8>);

impl Out {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend(v.to_be_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend(v.to_be_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend(v.to_be_bytes());
    }
    fn bytes(&mut self, v: &[u8]) {
        self.0.extend(v);
    }
    fn str(&mut self, v: &str) {
        self.u16(v.len() as u16);
        self.bytes(v.as_bytes());
    }
}

/// Big-endian bytes, read, failing past the end.
struct In<'a>(&'a [u8]);

impl<'a> In<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.0.len() < n {
            return Err(corrupt("the disc layout is cut short"));
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.bytes(N)?.try_into().unwrap())
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.array()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.array()?))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.array()?))
    }
    fn str(&mut self) -> Result<String> {
        let len = self.u16()? as usize;
        Ok(String::from_utf8_lossy(self.bytes(len)?).into_owned())
    }
}

impl Layout {
    /// The magic and version; the disc's size, CRC-32, MD5 and SHA-1; the
    /// game's folder; the disc header's sectors; then per partition its name,
    /// first and end sectors, header sectors, storage (0 none, 1 in the WUA,
    /// 2 in the store), stream SHA-1 and length, and regions: first sector,
    /// length, kind (0 content, 1 plain, 2 raw, 3 blocks) and index; then
    /// the sectors kept whole. Strings are a `u16` length and UTF-8.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Out::default();
        out.bytes(LAYOUT_MAGIC);
        out.u32(LAYOUT_VERSION);
        out.u64(self.info.size);
        out.u32(self.info.crc32);
        out.bytes(&self.info.md5);
        out.bytes(&self.info.sha1);
        out.str(&self.game);
        out.u64(self.header_sectors);
        out.u32(self.partitions.len() as u32);
        for p in &self.partitions {
            out.str(&p.name);
            out.u64(p.start);
            out.u64(p.end);
            out.u64(p.header_sectors);
            out.u8(match p.storage {
                Storage::None => 0,
                Storage::Embedded => 1,
                Storage::Store => 2,
            });
            out.bytes(&p.stream_sha1);
            out.u64(p.stream_len);
            out.u32(p.regions.len() as u32);
            for r in &p.regions {
                out.u64(r.start);
                out.u64(r.len);
                let (kind, index) = match r.kind {
                    Kind::Content(i) => (0, i),
                    Kind::Plain => (1, 0),
                    Kind::Raw(i) => (2, i),
                    Kind::Blocks(i) => (3, i),
                };
                out.u8(kind);
                out.u16(index);
            }
        }
        out.u32(self.patches.len() as u32);
        for &sector in &self.patches {
            out.u64(sector);
        }
        out.0
    }

    pub fn parse(bytes: &[u8]) -> Result<Layout> {
        let mut r = In(bytes);
        if r.bytes(4)? != LAYOUT_MAGIC {
            return Err(corrupt("the disc layout has no magic"));
        }
        if r.u32()? != LAYOUT_VERSION {
            return Err(Error::Unsupported(
                "a disc layout of a later version".into(),
            ));
        }
        let info = DiscInfo {
            size: r.u64()?,
            crc32: r.u32()?,
            md5: r.array()?,
            sha1: r.array()?,
        };
        let game = r.str()?;
        let header_sectors = r.u64()?;
        let mut partitions = Vec::new();
        for _ in 0..r.u32()? {
            let name = r.str()?;
            let (start, end, header_sectors) = (r.u64()?, r.u64()?, r.u64()?);
            let storage = match r.u8()? {
                0 => Storage::None,
                1 => Storage::Embedded,
                2 => Storage::Store,
                _ => return Err(corrupt("the disc layout has an unknown storage")),
            };
            let stream_sha1 = r.array()?;
            let stream_len = r.u64()?;
            let mut regions = Vec::new();
            for _ in 0..r.u32()? {
                let (start, len) = (r.u64()?, r.u64()?);
                let (kind, index) = (r.u8()?, r.u16()?);
                let kind = match kind {
                    0 => Kind::Content(index),
                    1 => Kind::Plain,
                    2 => Kind::Raw(index),
                    3 => Kind::Blocks(index),
                    _ => return Err(corrupt("the disc layout has an unknown region")),
                };
                regions.push(Region { start, len, kind });
            }
            if end < start {
                return Err(corrupt("the disc layout has a partition ending early"));
            }
            partitions.push(PartitionLayout {
                name,
                start,
                end,
                header_sectors,
                storage,
                stream_sha1,
                stream_len,
                regions,
            });
        }
        let mut patches = Vec::new();
        for _ in 0..r.u32()? {
            patches.push(r.u64()?);
        }
        Ok(Layout {
            info,
            game,
            header_sectors,
            partitions,
            patches,
        })
    }
}

pub(crate) fn transforms_to_bytes(transforms: &[Transform]) -> Vec<u8> {
    let mut out = Out::default();
    out.u32(transforms.len() as u32);
    for t in transforms {
        out.u64(t.offset);
        out.u64(t.len);
        out.u8(t.hashed as u8);
        out.u16(t.index);
        out.u64(t.ticket.0);
        out.u32(t.ticket.1);
        out.bytes(&t.common);
    }
    out.0
}

pub(crate) fn parse_transforms(bytes: &[u8]) -> Result<Vec<Transform>> {
    let mut r = In(bytes);
    (0..r.u32()?)
        .map(|_| {
            Ok(Transform {
                offset: r.u64()?,
                len: r.u64()?,
                hashed: r.u8()? != 0,
                index: r.u16()?,
                ticket: (r.u64()?, r.u32()?),
                common: r.array()?,
            })
        })
        .collect()
}

/// A region's bytes split where its titles' contents start and end, each
/// piece with the transform it is under, if any: `(offset in the region,
/// length, transform)`.
pub(crate) fn pieces(
    region_offset: u64,
    len: u64,
    transforms: &[Transform],
) -> Result<Vec<(u64, u64, Option<usize>)>> {
    let first = transforms.partition_point(|t| t.offset < region_offset);
    let mut pieces = Vec::new();
    let mut at = 0;
    for (i, t) in transforms.iter().enumerate().skip(first) {
        if t.offset >= region_offset + len {
            break;
        }
        let start = t.offset - region_offset;
        if start < at || start + t.len > len {
            return Err(corrupt("a title's content overlaps its region's end"));
        }
        if start > at {
            pieces.push((at, start - at, None));
        }
        pieces.push((start, t.len, Some(i)));
        at = start + t.len;
    }
    if len > at {
        pieces.push((at, len - at, None));
    }
    Ok(pieces)
}

/// Decrypt, or encrypt, `buf`, which is `at` bytes into the content `t`,
/// with its title key. `iv` carries a raw content's chain from one call to
/// the next; a hashed content is decrypted a block at a time, each block's
/// data with its hash as IV, so `at` and `buf` are whole blocks.
pub(crate) fn title_crypt(
    t: &Transform,
    key: &Key,
    at: u64,
    buf: &mut [u8],
    iv: &mut [u8; 16],
    decrypting: bool,
) {
    if !t.hashed {
        if at == 0 {
            *iv = disc::block_iv(t.index, 0);
        }
        match decrypting {
            true => cbc_decrypt(key, iv, buf),
            false => encrypt(key, iv, buf),
        }
        return;
    }
    for (i, block) in buf.chunks_mut(BLOCK as usize).enumerate() {
        let slot = ((at / BLOCK + i as u64) % 16) as usize * 20;
        let (area, data) = block.split_at_mut(HASHES);
        if decrypting {
            cbc_decrypt(key, &mut [0; 16], area);
            let mut iv: [u8; 16] = area[slot..slot + 16].try_into().unwrap();
            cbc_decrypt(key, &mut iv, data);
        } else {
            let mut iv: [u8; 16] = area[slot..slot + 16].try_into().unwrap();
            encrypt(key, &mut [0; 16], area);
            encrypt(key, &mut iv, data);
        }
    }
}

/// The title key in a ticket, a Wii U's or a vWii's, decrypted with
/// `common`.
pub(crate) fn ticket_key(ticket: &[u8], common: &Key) -> Result<Key> {
    if ticket.len() < 0x1F2 {
        return Err(corrupt("the ticket is cut short"));
    }
    let title_id = u64::from_be_bytes(ticket[0x1DC..0x1E4].try_into().unwrap());
    let encrypted: [u8; 16] = ticket[0x1BF..0x1CF].try_into().unwrap();
    Ok(decrypt_block(common, &ticket_iv(title_id), &encrypted))
}

/// The key among `keys` whose SHA-1 is `sha1`: the common key a title's
/// ticket was encrypted with, as its transforms record it.
pub(crate) fn common_by_sha1(keys: &[Key], sha1: &[u8; 20]) -> Result<Key> {
    keys.iter()
        .find(|key| Sha1::digest(key).as_slice() == sha1)
        .copied()
        .ok_or_else(|| {
            Error::MissingKey(
                "the common key of a title in the update partition, \
                 the vWii common key for a vWii title"
                    .into(),
            )
        })
}

/// Read `buf` of a region at `offset` in it, decrypted if the disc key
/// encrypts it. `offset` and `buf`'s length are multiples of 16.
fn read_plain(
    image: &Image,
    region_at: u64,
    kind: Kind,
    key: &Key,
    offset: u64,
    buf: &mut [u8],
) -> Result<()> {
    let iv_before = |at: u64| -> Result<[u8; 16]> {
        let mut iv = [0; 16];
        image.read_exact_at(&mut iv, region_at + at - 16)?;
        Ok(iv)
    };
    match kind {
        Kind::Content(_) | Kind::Plain => image.read_exact_at(buf, region_at + offset),
        Kind::Raw(index) => {
            let mut iv = match offset {
                0 => disc::block_iv(index, 0),
                _ => iv_before(offset)?,
            };
            image.read_exact_at(buf, region_at + offset)?;
            cbc_decrypt(key, &mut iv, buf);
            Ok(())
        }
        Kind::Blocks(index) => {
            let mut done = 0;
            while done < buf.len() {
                let at = offset + done as u64;
                let take = ((BLOCK - at % BLOCK) as usize).min(buf.len() - done);
                let mut iv = match at % BLOCK {
                    0 => disc::block_iv(index, at / BLOCK),
                    _ => iv_before(at)?,
                };
                let chunk = &mut buf[done..done + take];
                image.read_exact_at(chunk, region_at + at)?;
                cbc_decrypt(key, &mut iv, chunk);
                done += take;
            }
            Ok(())
        }
    }
}

/// A partition being collected: its layout, its titles' contents, and the
/// title keys of their tickets.
struct Part {
    layout: PartitionLayout,
    transforms: Vec<Transform>,
    keys: TitleKeys,
    tickets: Tickets,
}

/// Collect what rebuilds the disc at `disc`, whose game is the WUA's folder
/// `game`, into the `.wud/` folder of the WUA `writer` is writing, and the
/// update partition into `store` if given. `report` is called with the bytes
/// read: the disc's size, then each stream's.
pub(crate) fn write<W: Write>(
    writer: &mut Writer<W>,
    disc_path: &Path,
    game_folder: &str,
    keys: &[Key],
    store: Option<&Path>,
    report: &mut dyn FnMut(u64),
) -> Result<()> {
    let image = Rc::new(Image::open(disc_path)?);
    disc::check_magic(&image)?;
    if !image.size().is_multiple_of(SECTOR) {
        return Err(Error::Unsupported(
            "a disc whose size is not whole sectors".into(),
        ));
    }
    let disc_key = disc::find_disc_key(&image, keys)?;
    let partitions = disc::partitions(&image, &disc_key)?;
    let game = DiscGame::open(disc_path, keys)?;
    let common = {
        let location = disc::fst_location(&image, &partitions[game.index])?;
        let ticket = game.si_file("title.tik")?;
        let (title_id, encrypted) = parse_ticket(&ticket)?;
        wua::common_key(
            &disc::read_fst(&image, location)?,
            title_id,
            &encrypted,
            keys,
        )?
    };

    let total = image.size() / SECTOR;
    let mut parts = Vec::with_capacity(partitions.len());
    for (i, partition) in partitions.iter().enumerate() {
        if !partition.offset.is_multiple_of(SECTOR) {
            return Err(Error::Unsupported("a partition off a sector".into()));
        }
        let start = partition.offset / SECTOR;
        let end = partitions
            .get(i + 1)
            .map_or(total, |next| next.offset / SECTOR);
        if end < start || parts.last().is_some_and(|p: &Part| p.layout.end > start) {
            return Err(Error::Unsupported("partitions out of order".into()));
        }
        let header = disc::partition_header(&image, partition)?;
        let mut part = Part {
            layout: PartitionLayout {
                name: partition.name.clone(),
                start,
                end,
                header_sectors: header.sectors,
                storage: Storage::Embedded,
                stream_sha1: [0; 20],
                stream_len: 0,
                regions: Vec::new(),
            },
            transforms: Vec::new(),
            keys: HashMap::new(),
            tickets: HashMap::new(),
        };
        let source = PartitionSource {
            image: image.clone(),
            offset: partition.offset,
        };
        if i == game.index {
            part.layout.storage = Storage::None;
            part.layout.regions = content_regions(
                &game.fst,
                &game.si_file("title.tmd")?,
                header.sectors,
                Kind::Content,
            )?;
        } else if header.disc_key {
            let fst = Fst::open_in(
                &partition.name,
                disc::read_fst(&image, header.fst)?,
                header.fst.1,
                disc_key,
                header.hashed,
            )?;
            let mut regions = vec![Region {
                start: header.sectors,
                len: u64::from(header.fst.1).div_ceil(SECTOR) * SECTOR,
                kind: Kind::Raw(0),
            }];
            // Which region each cluster is, to place its files in the stream.
            let mut cluster_regions = HashMap::new();
            for (index, (offset, sectors, mode)) in fst.clusters().enumerate() {
                if sectors == 0 {
                    continue;
                }
                cluster_regions.insert(index as u16, regions.len());
                regions.push(Region {
                    start: offset / SECTOR,
                    len: sectors * SECTOR,
                    kind: match mode {
                        Mode::Raw => Kind::Raw(index as u16),
                        Mode::Blocks => Kind::Blocks(index as u16),
                        // Hashed under the disc key: kept as it is.
                        Mode::Hashed => Kind::Plain,
                    },
                });
            }
            let mut order: Vec<usize> = (0..regions.len()).collect();
            order.sort_by_key(|&r| regions[r].start);
            let sorted: Vec<Region> = order.iter().map(|&r| regions[r]).collect();
            part.layout.regions = sorted;
            let offsets = part.layout.stream_offsets();
            let cluster_stream = |cluster: u16| {
                let region = *cluster_regions.get(&cluster)?;
                let sorted = order.iter().position(|&r| r == region)?;
                offsets[sorted]
            };
            (part.transforms, part.keys, part.tickets) =
                find_transforms(&fst, &source, &cluster_stream, &common, keys)?;
        } else {
            // Another game partition: its regions as they are.
            let ticket = si_file(&game, i, "title.tik");
            let tmd = si_file(&game, i, "title.tmd");
            let regions = match (ticket, tmd) {
                (Some(ticket), Some(tmd)) => {
                    let title_key = ticket_key(&ticket, &common)?;
                    Fst::open(
                        &partition.name,
                        disc::read_fst(&image, header.fst)?,
                        header.fst.1,
                        title_key,
                    )
                    .and_then(|fst| content_regions(&fst, &tmd, header.sectors, |_| Kind::Plain))
                    .ok()
                }
                _ => None,
            };
            // Without its title's TMD, the whole partition is kept.
            part.layout.regions = regions.unwrap_or_else(|| {
                vec![Region {
                    start: header.sectors,
                    len: (end - start - header.sectors) * SECTOR,
                    kind: Kind::Plain,
                }]
            });
        }
        part.layout.steps()?;
        part.layout.stream_len = part
            .layout
            .regions
            .iter()
            .filter(|r| !matches!(r.kind, Kind::Content(_)))
            .map(|r| r.len)
            .sum();
        parts.push(part);
    }
    let first = partitions.first().map_or(total, |p| p.offset / SECTOR);

    let (info, patches) = check(&image, first, &parts, report)?;

    let dir = writer.dir(zarchive::ROOT, DISC_DIR.as_bytes())?;
    // The disc's header, then each partition's.
    writer.file(dir, b"header")?;
    let mut buf = vec![0; SECTOR as usize];
    for s in 0..first {
        image.read_exact_at(&mut buf, s * SECTOR)?;
        writer.append(&buf)?;
    }
    for part in &parts {
        for s in 0..part.layout.header_sectors {
            image.read_exact_at(&mut buf, (part.layout.start + s) * SECTOR)?;
            writer.append(&buf)?;
        }
    }
    writer.file(dir, b"patches")?;
    for (_, bytes) in &patches {
        writer.append(bytes)?;
    }
    // The streams count for the disc's size, as `to_wua` budgets them, though
    // they leave out the game's contents and the disc's padding: reported as
    // they are, they would fall far short, and the rest come in one jump.
    let mut streams = Scaled::new(
        &mut *report,
        parts
            .iter()
            .filter(|p| p.layout.storage != Storage::None)
            .map(|p| p.layout.stream_len)
            .sum(),
        image.size(),
    );
    let mut partition_layouts = Vec::with_capacity(parts.len());
    for (i, mut part) in parts.into_iter().enumerate() {
        if part.layout.storage == Storage::None {
            partition_layouts.push(part.layout);
            continue;
        }
        let in_store = store.filter(|_| part.layout.name.starts_with("UP"));
        // Reported as it is read: an update partition is over a gigabyte.
        let mut reported = 0;
        if let Some(store) = in_store {
            part.layout.storage = Storage::Store;
            // Its SHA-1 first, which names it: the store may have it already.
            // Each of the two reads counts for half.
            let mut sha1 = Sha1::new();
            let mut half = |chunk: &[u8]| {
                let n = chunk.len() as u64 / 2;
                reported += n;
                streams.add(n);
                Ok(())
            };
            emit(&image, &part, &disc_key, false, &mut half, &mut sha1)?;
            part.layout.stream_sha1 = sha1.finalize().into();
            let path = store.join(store_name(&part.layout.stream_sha1));
            if !path.is_file() {
                std::fs::create_dir_all(store)?;
                write_entry(&image, &part, &disc_key, &path, &mut half)?;
            }
        } else {
            let sub = writer.dir(dir, format!("{i:02}").as_bytes())?;
            writer.file(sub, b"stream")?;
            let mut sha1 = Sha1::new();
            emit(
                &image,
                &part,
                &disc_key,
                true,
                &mut |chunk| {
                    reported += chunk.len() as u64;
                    streams.add(chunk.len() as u64);
                    writer.append(chunk)
                },
                &mut sha1,
            )?;
            part.layout.stream_sha1 = sha1.finalize().into();
            writer.file(sub, b"transforms")?;
            writer.append(&transforms_to_bytes(&part.transforms))?;
        }
        streams.add(part.layout.stream_len.saturating_sub(reported));
        partition_layouts.push(part.layout);
    }
    streams.finish();
    let layout = Layout {
        info,
        game: game_folder.to_string(),
        header_sectors: first,
        partitions: partition_layouts,
        patches: patches.iter().map(|(sector, _)| *sector).collect(),
    };
    writer.file(dir, b"layout")?;
    writer.append(&layout.to_bytes())?;
    Ok(())
}

/// A store entry's file name: the SHA-1 of the stream it holds.
pub(crate) fn store_name(sha1: &[u8; 20]) -> String {
    let hex: String = sha1.iter().map(|b| format!("{b:02x}")).collect();
    format!("{hex}.zst")
}

/// A store entry's head, before its stream: the magic and version, its
/// transforms, and the tickets they need, which the stream may hold only
/// after the contents they decrypt, each its offset, length and bytes.
pub(crate) fn entry_head(transforms: &[Transform], tickets: &Tickets) -> Vec<u8> {
    let mut out = Out::default();
    out.bytes(ENTRY_MAGIC);
    out.u32(ENTRY_VERSION);
    let transforms = transforms_to_bytes(transforms);
    out.u32(transforms.len() as u32);
    out.bytes(&transforms);
    let mut tickets: Vec<_> = tickets.iter().collect();
    tickets.sort();
    out.u32(tickets.len() as u32);
    for ((offset, len), bytes) in tickets {
        out.u64(*offset);
        out.u32(*len);
        out.bytes(bytes);
    }
    out.0
}

/// Read a store entry's head from `reader`, leaving it at the stream.
pub(crate) fn read_entry_head(
    reader: &mut impl std::io::Read,
) -> Result<(Vec<Transform>, Tickets)> {
    let mut read = |n: usize| -> Result<Vec<u8>> {
        let mut bytes = vec![0; n];
        reader.read_exact(&mut bytes)?;
        Ok(bytes)
    };
    let u32_of = |b: Vec<u8>| u32::from_be_bytes(b.try_into().unwrap());
    if read(4)? != ENTRY_MAGIC {
        return Err(corrupt("the store entry has no magic"));
    }
    if u32_of(read(4)?) != ENTRY_VERSION {
        return Err(Error::Unsupported(
            "a store entry of a later version".into(),
        ));
    }
    let len = u32_of(read(4)?) as usize;
    let transforms = parse_transforms(&read(len)?)?;
    let mut tickets = HashMap::new();
    for _ in 0..u32_of(read(4)?) {
        let offset = u64::from_be_bytes(read(8)?.try_into().unwrap());
        let len = u32_of(read(4)?);
        tickets.insert((offset, len), read(len as usize)?);
    }
    Ok((transforms, tickets))
}

/// Write a partition's stream as a store entry: its head, then the stream,
/// all one zstd stream.
fn write_entry(
    image: &Image,
    part: &Part,
    disc_key: &Key,
    path: &Path,
    report: &mut dyn FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    // Another export of a disc with the same update may be writing it too:
    // each writes its own, and either's rename leaves the same entry.
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.part", std::process::id()));
    let part_path = path.with_file_name(name);
    let result = (|| {
        let file = std::fs::File::create(&part_path)?;
        let mut encoder = zstd::Encoder::new(file, ENTRY_LEVEL)?;
        encoder.long_distance_matching(true)?;
        encoder.window_log(ENTRY_WINDOW_LOG)?;
        let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
        encoder.multithread(workers as u32)?;
        encoder.write_all(&entry_head(&part.transforms, &part.tickets))?;
        let mut sha1 = Sha1::new();
        emit(
            image,
            part,
            disc_key,
            true,
            &mut |chunk| {
                encoder.write_all(chunk)?;
                report(chunk)
            },
            &mut sha1,
        )?;
        encoder.finish()?.sync_all()?;
        Ok(())
    })();
    finish(&part_path, path, result)
}

/// Hand a partition's stream to `sink`: its regions but the game's
/// contents, decrypted if the disc key encrypts them, and, if `transform`,
/// its titles' contents decrypted with their title keys. `sha1` hashes it
/// before that.
fn emit(
    image: &Image,
    part: &Part,
    disc_key: &Key,
    transform: bool,
    sink: &mut dyn FnMut(&[u8]) -> Result<()>,
    sha1: &mut Sha1,
) -> Result<()> {
    let offsets = part.layout.stream_offsets();
    let mut buf = vec![0; CHUNK as usize];
    for (region, offset) in part.layout.regions.iter().zip(offsets) {
        let Some(offset) = offset else { continue };
        let region_at = (part.layout.start + region.start) * SECTOR;
        for (at, len, t) in pieces(offset, region.len, &part.transforms)? {
            let mut iv = [0; 16];
            let mut done = 0;
            while done < len {
                let n = (len - done).min(CHUNK) as usize;
                let chunk = &mut buf[..n];
                read_plain(image, region_at, region.kind, disc_key, at + done, chunk)?;
                sha1.update(&*chunk);
                if let (Some(t), true) = (t, transform) {
                    let t = &part.transforms[t];
                    title_crypt(t, &part.keys[&t.ticket], done, chunk, &mut iv, true);
                }
                sink(chunk)?;
                done += n as u64;
            }
        }
    }
    Ok(())
}

/// A partition's regions, from the TMD of its title and its FST: content 0,
/// the FST, where the partition's header says, the others at their
/// clusters, each as long as the TMD says.
fn content_regions(
    fst: &Fst,
    tmd: &[u8],
    fst_sector: u64,
    kind: impl Fn(u16) -> Kind,
) -> Result<Vec<Region>> {
    let tmd = nus::parse_tmd(tmd)?;
    let mut regions = tmd
        .contents
        .iter()
        .map(|content| {
            let start = match content.index {
                0 => fst_sector,
                index => {
                    fst.cluster_offset(index)
                        .ok_or_else(|| corrupt(format!("the FST has no cluster {index}")))?
                        / SECTOR
                }
            };
            Ok(Region {
                start,
                len: content.size,
                kind: kind(content.index),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    regions.sort_by_key(|r| r.start);
    Ok(regions)
}

fn si_file(game: &DiscGame, index: usize, name: &str) -> Option<Vec<u8>> {
    let path = format!("{index:02x}/{name}");
    let file = game.si_fst.find(&path)?;
    game.si_fst.read(&game.si_source, file).ok()
}

/// The titles in a partition whose contents can be decrypted with their
/// title key: those whose folder holds their TMD and ticket, and `.app`s
/// as long as the TMD says. `stream` places a cluster in the stream.
fn find_transforms(
    fst: &Fst,
    source: &PartitionSource,
    stream: &dyn Fn(u16) -> Option<u64>,
    common: &Key,
    candidates: &[Key],
) -> Result<(Vec<Transform>, TitleKeys, Tickets)> {
    let items = tree(fst);
    let mut by_dir: HashMap<usize, HashMap<Vec<u8>, usize>> = HashMap::new();
    for item in items.iter().filter(|i| !i.is_dir) {
        by_dir
            .entry(item.parent)
            .or_default()
            .insert(fst.name(item.entry)?.to_ascii_lowercase(), item.entry);
    }
    let place = |entry: usize| -> Option<u64> {
        let (cluster, offset, _) = fst.extent(entry);
        Some(stream(cluster)? + offset)
    };
    let mut transforms = Vec::new();
    let mut keys = HashMap::new();
    let mut tickets = HashMap::new();
    // The vWii common key, once a vWii title has shown which it is.
    let mut vwii: Option<Key> = None;
    for files in by_dir.values() {
        let (Some(&tmd), Some(&tik)) = (files.get(&b"title.tmd"[..]), files.get(&b"title.tik"[..]))
        else {
            continue;
        };
        let (Ok(tmd_bytes), Ok(ticket)) = (fst.read(source, tmd), fst.read(source, tik)) else {
            continue;
        };
        // A vWii title's ticket names the vWii common key, its TMD a Wii's.
        let is_vwii = ticket.get(0x1F1) == Some(&2);
        let tmd = match is_vwii {
            true => nus::parse_wii_tmd(&tmd_bytes),
            false => nus::parse_tmd(&tmd_bytes),
        };
        let (Ok(tmd), Some(ticket_at)) = (tmd, place(tik)) else {
            continue;
        };
        if ticket.len() < 0x1F2
            || tmd.title_id != u64::from_be_bytes(ticket[0x1DC..0x1E4].try_into().unwrap())
        {
            continue;
        }
        let common = match is_vwii {
            false => *common,
            true => match find_vwii_common(fst, source, files, &tmd, &ticket, vwii, candidates)? {
                Some(key) => {
                    vwii = Some(key);
                    key
                }
                // Without it, the title is kept as it is.
                None => continue,
            },
        };
        let key = ticket_key(&ticket, &common)?;
        let common_sha1: [u8; 20] = Sha1::digest(common).into();
        let mut found = Vec::new();
        let mut valid = true;
        for content in &tmd.contents {
            let name = format!("{:08x}.app", content.id).into_bytes();
            let Some(&entry) = files.get(&name) else {
                continue;
            };
            let hashed = content.is_hashed();
            let size = fst.extent(entry).2;
            let Some(at) = place(entry) else { continue };
            // A raw content's file is padded to the AES block size.
            let expected = match hashed {
                true => content.size,
                false => content.size.next_multiple_of(16),
            };
            if size != expected
                || size == 0
                || !size.is_multiple_of(if hashed { BLOCK } else { 16 })
                || !at.is_multiple_of(16)
            {
                continue;
            }
            // A hashed content's first block must check against its hash
            // once decrypted, or the key isn't its title's.
            if hashed {
                let mut block = Vec::with_capacity(BLOCK as usize);
                let mut first = true;
                fst.read_with(source, entry, &mut |chunk| {
                    if first {
                        let take = (BLOCK as usize - block.len()).min(chunk.len());
                        block.extend_from_slice(&chunk[..take]);
                        first = block.len() < BLOCK as usize;
                    }
                    Ok(())
                })?;
                let (area, data) = block.split_at_mut(HASHES);
                cbc_decrypt(&key, &mut [0; 16], area);
                let mut iv: [u8; 16] = area[..16].try_into().unwrap();
                cbc_decrypt(&key, &mut iv, data);
                if Sha1::digest(&*data).as_slice() != &area[..20] {
                    valid = false;
                    break;
                }
            }
            found.push(Transform {
                offset: at,
                len: size,
                hashed,
                index: content.index,
                ticket: (ticket_at, ticket.len() as u32),
                common: common_sha1,
            });
        }
        if valid && !found.is_empty() {
            let at = (ticket_at, ticket.len() as u32);
            keys.insert(at, key);
            tickets.insert(at, ticket);
            transforms.extend(found);
        }
    }
    transforms.sort_by_key(|t| t.offset);
    Ok((transforms, keys, tickets))
}

/// The vWii common key among `candidates`, `known` first: the one whose
/// title key, from the vWii title's `ticket`, decrypts the title's smallest
/// content to what its Wii TMD hashes.
fn find_vwii_common(
    fst: &Fst,
    source: &PartitionSource,
    files: &HashMap<Vec<u8>, usize>,
    tmd: &nus::Tmd,
    ticket: &[u8],
    known: Option<Key>,
    candidates: &[Key],
) -> Result<Option<Key>> {
    let smallest = tmd
        .contents
        .iter()
        .filter(|c| c.size > 0)
        .filter_map(|c| {
            let entry = *files.get(format!("{:08x}.app", c.id).as_bytes())?;
            Some((c, entry))
        })
        .min_by_key(|(c, _)| c.size);
    let Some((content, entry)) = smallest else {
        return Ok(None);
    };
    let encrypted = fst.read(source, entry)?;
    if (encrypted.len() as u64) < content.size || !encrypted.len().is_multiple_of(16) {
        return Ok(None);
    }
    let opens = |common: &Key| -> Result<bool> {
        let title_key = ticket_key(ticket, common)?;
        let mut plain = encrypted.clone();
        cbc_decrypt(
            &title_key,
            &mut disc::block_iv(content.index, 0),
            &mut plain,
        );
        Ok(Sha1::digest(&plain[..content.size as usize]).as_slice() == &content.hash[..20])
    };
    for common in known.iter().chain(candidates) {
        if opens(common)? {
            return Ok(Some(*common));
        }
    }
    Ok(None)
}

/// Read the disc through: its hashes, and the sectors the layout gets
/// wrong, kept whole. `report` is called with the bytes read.
fn check(
    image: &Image,
    first: u64,
    parts: &[Part],
    report: &mut dyn FnMut(u64),
) -> Result<(DiscInfo, Patches)> {
    // What each sector should be past the disc header: a repeat of an
    // earlier one, or a content's last sector, zero past its end.
    let total = image.size() / SECTOR;
    let mut repeats: HashMap<u64, Option<u64>> = HashMap::new();
    let mut tails: HashMap<u64, usize> = HashMap::new();
    for part in parts {
        let mut cursor = part.layout.start;
        for step in part.layout.steps()? {
            match step {
                Step::Header => cursor += part.layout.header_sectors,
                Step::Region(i) => {
                    let region = &part.layout.regions[i];
                    let rest = (region.len % SECTOR) as usize;
                    if rest != 0 {
                        tails.insert(
                            part.layout.start + region.start + region.sectors() - 1,
                            rest,
                        );
                    }
                    cursor = part.layout.start + region.start + region.sectors();
                }
                Step::Fill { count, period } => {
                    for s in cursor..cursor + count {
                        repeats.insert(s, period.map(|p| s - p));
                    }
                    cursor += count;
                }
            }
        }
    }
    let table = image.table();
    let (mut sha1, mut md5, mut crc) = (Sha1::new(), Md5::new(), crc32fast::Hasher::new());
    let mut patches = Vec::new();
    let batch = 32;
    let mut buf = vec![0; (batch * SECTOR) as usize];
    let mut earlier = vec![0; SECTOR as usize];
    let mut s = 0;
    while s < total {
        let n = (total - s).min(batch);
        let chunk = &mut buf[..(n * SECTOR) as usize];
        image.read_exact_at(chunk, s * SECTOR)?;
        rayon::join(
            || md5.update(&*chunk),
            || {
                sha1.update(&*chunk);
                crc.update(chunk);
            },
        );
        for (i, sector) in chunk.chunks(SECTOR as usize).enumerate() {
            let at = s + i as u64;
            if at < first {
                continue;
            }
            let wrong = match (repeats.get(&at), tails.get(&at)) {
                (Some(None), _) => true,
                (Some(Some(from)), _) => match table {
                    Some(table) => table[at as usize] != table[*from as usize],
                    None => {
                        image.read_exact_at(&mut earlier, from * SECTOR)?;
                        earlier != sector
                    }
                },
                (None, Some(&rest)) => sector[rest..].iter().any(|&b| b != 0),
                (None, None) => false,
            };
            if wrong {
                patches.push((at, sector.to_vec()));
            }
        }
        report(n * SECTOR);
        s += n;
    }
    Ok((
        DiscInfo {
            size: image.size(),
            crc32: crc.finalize(),
            md5: md5.finalize().into(),
            sha1: sha1.finalize().into(),
        },
        patches,
    ))
}

/// The store entries a WUA's disc needs, by file name: its update
/// partition's, unless the WUA holds it.
pub fn store_entries(wua: &Path) -> Result<Vec<String>> {
    let mut reader = zarchive::Reader::open(wua)?;
    let Some(layout) = reader.lookup(zarchive::ROOT, &format!("{DISC_DIR}/layout")) else {
        return Ok(Vec::new());
    };
    Ok(Layout::parse(&reader.read_file(layout)?)?
        .partitions
        .iter()
        .filter(|p| p.storage == Storage::Store)
        .map(|p| store_name(&p.stream_sha1))
        .collect())
}

/// The disc a WUA rebuilds, if it was written with what rebuilds it.
pub fn disc_info(wua: &Path) -> Result<Option<DiscInfo>> {
    let mut reader = zarchive::Reader::open(wua)?;
    let Some(layout) = reader.lookup(zarchive::ROOT, &format!("{DISC_DIR}/layout")) else {
        return Ok(None);
    };
    Ok(Some(Layout::parse(&reader.read_file(layout)?)?.info))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_survive_their_bytes() {
        let layout = Layout {
            info: DiscInfo {
                size: 0x8000 * 64,
                crc32: 0x1234_5678,
                md5: [7; 16],
                sha1: [9; 20],
            },
            game: "0005000010101010_v32".into(),
            header_sectors: 4,
            partitions: vec![PartitionLayout {
                name: "UP".into(),
                start: 4,
                end: 64,
                header_sectors: 1,
                storage: Storage::Store,
                stream_sha1: [3; 20],
                stream_len: 0x18000,
                regions: vec![
                    Region {
                        start: 1,
                        len: 0x8000,
                        kind: Kind::Raw(0),
                    },
                    Region {
                        start: 3,
                        len: 0x10000,
                        kind: Kind::Blocks(1),
                    },
                ],
            }],
            patches: vec![9, 40],
        };
        let parsed = Layout::parse(&layout.to_bytes()).unwrap();
        assert_eq!(parsed.to_bytes(), layout.to_bytes());
        assert_eq!(
            parsed.partitions[0].steps().unwrap(),
            vec![
                Step::Header,
                Step::Region(0),
                Step::Fill {
                    count: 1,
                    period: Some(1)
                },
                Step::Region(1),
                Step::Fill {
                    count: 55,
                    period: Some(2)
                },
            ]
        );
        let transforms = vec![Transform {
            offset: 0x8000,
            len: 0x10000,
            hashed: true,
            index: 3,
            ticket: (16, 0x2A4),
            common: [5; 20],
        }];
        assert_eq!(
            parse_transforms(&transforms_to_bytes(&transforms)).unwrap(),
            transforms
        );
    }

    #[test]
    fn pieces_split_regions_at_contents() {
        let t = |offset, len| Transform {
            offset,
            len,
            hashed: false,
            index: 1,
            ticket: (0, 0),
            common: [0; 20],
        };
        let transforms = vec![t(0x100, 0x20), t(0x200, 0x30), t(0x2000, 0x10)];
        assert_eq!(
            pieces(0x80, 0x400, &transforms).unwrap(),
            vec![
                (0, 0x80, None),
                (0x80, 0x20, Some(0)),
                (0xA0, 0xE0, None),
                (0x180, 0x30, Some(1)),
                (0x1B0, 0x250, None),
            ]
        );
    }
}

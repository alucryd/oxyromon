//! What a disc is made of, as the Wii U's mastering lays it out: each
//! partition's header, its regions, the FST and the clusters or contents it
//! lists, how the disc key and title keys encrypt them, and the titles in an
//! update partition whose contents decrypt with their own keys. wux-rs's
//! lossless WUA and ore-rs's ORE are both written from it.

use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use sha1::{Digest, Sha1};

use super::crypto::{Key, cbc_decrypt, decrypt_block, encrypt};
use super::disc::{self, Fst, Mode, PartitionSource, SECTOR};
use super::error::{Error, Result, corrupt};
use super::image::Image;
use super::nus;

/// A 64 KiB block, of a hashed or blocks cluster, or of a hashed content.
pub const BLOCK: u64 = 0x10000;
/// A hashed block's hashes, before its data.
pub const HASHES: usize = 0x400;

/// A disc's game partition, opened: its SI partition, which holds its TMD,
/// ticket and certificates, and its FST, with the title key its files
/// decrypt with. Like Cemu, the first game (GM) partition.
pub struct DiscGame {
    pub si_fst: Fst,
    pub si_source: PartitionSource,
    /// The game partition's index, which names its folder in the SI partition.
    pub index: usize,
    pub fst: Fst,
    /// Where the FST starts in the game partition: content 0, which the FST
    /// itself places at the partition's start.
    pub fst_start: u64,
    pub source: PartitionSource,
    pub title_key: Key,
}

impl DiscGame {
    pub fn open(input: &Path, keys: &[Key]) -> Result<DiscGame> {
        let image = Rc::new(Image::open(input)?);
        disc::check_magic(&image)?;
        let disc_key = disc::find_disc_key(&image, keys)?;
        let partitions = disc::partitions(&image, &disc_key)?;
        let find = |prefix: &str| {
            partitions
                .iter()
                .position(|p| p.name.starts_with(prefix))
                .ok_or_else(|| corrupt(format!("the disc has no {prefix} partition")))
        };
        let (si, gm) = (find("SI")?, find("GM")?);

        let header = disc::partition_header(&image, &partitions[si])?;
        let si_fst = Fst::open_in(
            "the SI partition",
            disc::read_fst(&image, header.fst)?,
            header.fst.1,
            disc_key,
            header.hashed,
        )?;
        let si_source = PartitionSource {
            image: image.clone(),
            offset: partitions[si].offset,
        };
        let ticket = si_file(&si_fst, &si_source, gm, "title.tik")?;
        let (title_id, encrypted_key) = parse_ticket(&ticket)?;
        let location = disc::fst_location(&image, &partitions[gm])?;
        let encrypted = disc::read_fst(&image, location)?;
        let title_key = title_key(&encrypted, title_id, &encrypted_key, keys)?;
        let fst = Fst::open("the game partition", encrypted, location.1, title_key)?;
        Ok(DiscGame {
            si_fst,
            si_source,
            index: gm,
            fst,
            fst_start: location.0 - partitions[gm].offset,
            source: PartitionSource {
                image,
                offset: partitions[gm].offset,
            },
            title_key,
        })
    }

    /// The game partition's file `name` in the SI partition: its `title.tmd`,
    /// `title.tik` or `title.cert`.
    pub fn si_file(&self, name: &str) -> Result<Vec<u8>> {
        si_file(&self.si_fst, &self.si_source, self.index, name)
    }
}

pub fn si_file(fst: &Fst, source: &PartitionSource, index: usize, name: &str) -> Result<Vec<u8>> {
    let path = format!("{index:02x}/{name}");
    let file = fst
        .find(&path)
        .ok_or_else(|| corrupt(format!("the SI partition has no {path}")))?;
    fst.read(source, file)
}

/// The title ID and encrypted title key in a ticket.
pub fn parse_ticket(ticket: &[u8]) -> Result<(u64, [u8; 16])> {
    if ticket.len() < 0x1E8 {
        return Err(corrupt("the ticket is cut short"));
    }
    let title_id = u64::from_be_bytes(ticket[0x1DC..0x1E4].try_into().unwrap());
    if title_id >> 48 != 5 {
        return Err(corrupt(format!("the ticket's title ID is {title_id:016x}")));
    }
    Ok((title_id, ticket[0x1BF..0x1CF].try_into().unwrap()))
}

/// The title key, decrypted by whichever of `keys` is the common key: the one
/// whose title key decrypts the title's FST.
pub fn title_key(fst: &[u8], title_id: u64, encrypted_key: &[u8; 16], keys: &[Key]) -> Result<Key> {
    let common = common_key(fst, title_id, encrypted_key, keys)?;
    Ok(decrypt_block(&common, &ticket_iv(title_id), encrypted_key))
}

/// Which of `keys` is the Wii U common key: the one whose title key, from
/// the ticket, decrypts the title's FST.
pub fn common_key(
    fst: &[u8],
    title_id: u64,
    encrypted_key: &[u8; 16],
    keys: &[Key],
) -> Result<Key> {
    let iv = ticket_iv(title_id);
    keys.iter()
        .find(|common| disc::opens_fst(fst, &decrypt_block(common, &iv, encrypted_key)))
        .copied()
        .ok_or_else(|| Error::MissingKey("none of the keys is the Wii U common key".into()))
}

/// The IV a title key is encrypted with: the title ID.
pub fn ticket_iv(title_id: u64) -> [u8; 16] {
    let mut iv = [0; 16];
    iv[..8].copy_from_slice(&title_id.to_be_bytes());
    iv
}

/// An FST entry to write, under the directory at FST index `parent` (0 for
/// the root).
pub struct Item {
    pub entry: usize,
    pub parent: usize,
    pub is_dir: bool,
}

/// The FST's entries in order, links and what's under them left out.
pub fn tree(fst: &Fst) -> Vec<Item> {
    let mut items = Vec::new();
    // Directories we're in: their index and the index past their end.
    let mut stack = vec![(0, fst.entries.len())];
    let mut i = 1;
    while i < fst.entries.len() {
        while i >= stack.last().unwrap().1 {
            stack.pop();
        }
        let entry = &fst.entries[i];
        if entry.is_link() {
            i = if entry.is_dir() { entry.end() } else { i + 1 };
            continue;
        }
        items.push(Item {
            entry: i,
            parent: stack.last().unwrap().0,
            is_dir: entry.is_dir(),
        });
        if entry.is_dir() {
            stack.push((i, entry.end()));
        }
        i += 1;
    }
    items
}

/// Where a region's bytes come from, and how they are encrypted on the disc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
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
pub struct Region {
    pub start: u64,
    pub len: u64,
    pub kind: Kind,
}

impl Region {
    pub fn sectors(&self) -> u64 {
        self.len.div_ceil(SECTOR)
    }
}

/// Tickets, and their title keys, by where they are in a stream.
pub type Tickets = HashMap<(u64, u32), Vec<u8>>;

pub type TitleKeys = HashMap<(u64, u32), Key>;

/// A title's content, in a stream decrypted with its title key: where it
/// is, what it is, where its ticket is, which holds that key, and the SHA-1
/// of the common key that key is encrypted with: the Wii U's, or for a vWii
/// title, the vWii's, which a rebuild finds among its keys by that SHA-1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transform {
    pub offset: u64,
    pub len: u64,
    pub hashed: bool,
    pub index: u16,
    pub ticket: (u64, u32),
    pub common: [u8; 20],
}

/// A region's bytes split where its titles' contents start and end, each
/// piece with the transform it is under, if any: `(offset in the region,
/// length, transform)`.
pub fn pieces(
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
pub fn title_crypt(
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
pub fn ticket_key(ticket: &[u8], common: &Key) -> Result<Key> {
    if ticket.len() < 0x1F2 {
        return Err(corrupt("the ticket is cut short"));
    }
    let title_id = u64::from_be_bytes(ticket[0x1DC..0x1E4].try_into().unwrap());
    let encrypted: [u8; 16] = ticket[0x1BF..0x1CF].try_into().unwrap();
    Ok(decrypt_block(common, &ticket_iv(title_id), &encrypted))
}

/// The key among `keys` whose SHA-1 is `sha1`: the common key a title's
/// ticket was encrypted with, as its transforms record it.
pub fn common_by_sha1(keys: &[Key], sha1: &[u8; 20]) -> Result<Key> {
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
pub fn read_plain(
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

/// A partition's regions, from the TMD of its title and its FST: content 0,
/// the FST, where the partition's header says, the others at their
/// clusters, each as long as the TMD says.
pub fn content_regions(
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

pub fn si_partition_file(game: &DiscGame, index: usize, name: &str) -> Option<Vec<u8>> {
    let path = format!("{index:02x}/{name}");
    let file = game.si_fst.find(&path)?;
    game.si_fst.read(&game.si_source, file).ok()
}

/// The titles in a partition whose contents can be decrypted with their
/// title key: those whose folder holds their TMD and ticket, and `.app`s
/// as long as the TMD says. `stream` places a cluster in the stream.
pub fn find_transforms(
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
            let expected = content.stored_size();
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
pub fn find_vwii_common(
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

/// Each stream region's offset in its partition's stream, by region index:
/// the regions but the game's contents, end to end.
pub fn stream_offsets(regions: &[Region]) -> Vec<Option<u64>> {
    let mut at = 0;
    regions
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

/// A partition: where it is, in sectors, its header, its regions in order,
/// and in a disc-key partition the titles whose contents its title keys
/// decrypt, placed in its stream (see [`stream_offsets`]).
pub struct PartitionPlan {
    pub name: String,
    /// Its first sector on the disc, and the next partition's, or the end.
    pub start: u64,
    pub end: u64,
    pub header_sectors: u64,
    /// The game partition, whose regions are its game's contents.
    pub game: bool,
    pub regions: Vec<Region>,
    pub transforms: Vec<Transform>,
    pub keys: TitleKeys,
    pub tickets: Tickets,
}

/// A disc, laid out: its image, keys, game, the sectors before its first
/// partition, and its partitions.
pub struct Plan {
    pub image: Rc<Image>,
    pub disc_key: Key,
    /// The Wii U common key, which the game's ticket is encrypted with.
    pub common: Key,
    pub game: DiscGame,
    pub header_sectors: u64,
    pub partitions: Vec<PartitionPlan>,
}

/// Lay out the disc at `disc_path`, opened with `keys`: they must hold its
/// disc key and the Wii U common key, and, for a vWii title in its update
/// partition to be decrypted, the vWii common key.
pub fn plan(disc_path: &Path, keys: &[Key]) -> Result<Plan> {
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
        common_key(
            &disc::read_fst(&image, location)?,
            title_id,
            &encrypted,
            keys,
        )?
    };

    let total = image.size() / SECTOR;
    let mut parts: Vec<PartitionPlan> = Vec::with_capacity(partitions.len());
    for (i, partition) in partitions.iter().enumerate() {
        if !partition.offset.is_multiple_of(SECTOR) {
            return Err(Error::Unsupported("a partition off a sector".into()));
        }
        let start = partition.offset / SECTOR;
        let end = partitions
            .get(i + 1)
            .map_or(total, |next| next.offset / SECTOR);
        if end < start || parts.last().is_some_and(|p| p.end > start) {
            return Err(Error::Unsupported("partitions out of order".into()));
        }
        let header = disc::partition_header(&image, partition)?;
        let mut part = PartitionPlan {
            name: partition.name.clone(),
            start,
            end,
            header_sectors: header.sectors,
            game: i == game.index,
            regions: Vec::new(),
            transforms: Vec::new(),
            keys: HashMap::new(),
            tickets: HashMap::new(),
        };
        let source = PartitionSource {
            image: image.clone(),
            offset: partition.offset,
        };
        if part.game {
            part.regions = content_regions(
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
            part.regions = order.iter().map(|&r| regions[r]).collect();
            let offsets = stream_offsets(&part.regions);
            let cluster_stream = |cluster: u16| {
                let region = *cluster_regions.get(&cluster)?;
                let sorted = order.iter().position(|&r| r == region)?;
                offsets[sorted]
            };
            (part.transforms, part.keys, part.tickets) =
                find_transforms(&fst, &source, &cluster_stream, &common, keys)?;
        } else {
            // Another game partition: its regions as they are.
            let ticket = si_partition_file(&game, i, "title.tik");
            let tmd = si_partition_file(&game, i, "title.tmd");
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
            part.regions = regions.unwrap_or_else(|| {
                vec![Region {
                    start: header.sectors,
                    len: (end - start - header.sectors) * SECTOR,
                    kind: Kind::Plain,
                }]
            });
        }
        parts.push(part);
    }
    let header_sectors = partitions.first().map_or(total, |p| p.offset / SECTOR);
    Ok(Plan {
        image,
        disc_key,
        common,
        game,
        header_sectors,
        partitions: parts,
    })
}

/// What makes up a partition, sector by sector: its header, its regions,
/// and runs repeating the region before them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Header,
    Region(usize),
    /// `count` sectors, each the one `period` sectors before it; `None` if
    /// nothing comes before, which only patches can fill.
    Fill {
        count: u64,
        period: Option<u64>,
    },
}

/// A partition's steps, covering its `size` sectors whole: its header, its
/// regions, and between them runs repeating the region before.
pub fn steps(name: &str, header_sectors: u64, regions: &[Region], size: u64) -> Result<Vec<Step>> {
    let mut steps = Vec::new();
    let mut cursor = header_sectors;
    let mut period = None;
    if header_sectors > 0 {
        steps.push(Step::Header);
        period = Some(header_sectors);
    }
    for (i, region) in regions.iter().enumerate() {
        if region.start < cursor || region.start + region.sectors() > size {
            return Err(corrupt(format!(
                "partition {}'s regions overlap or overflow it",
                name
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

impl PartitionPlan {
    /// The partition's steps, covering it whole.
    pub fn steps(&self) -> Result<Vec<Step>> {
        steps(
            &self.name,
            self.header_sectors,
            &self.regions,
            self.end - self.start,
        )
    }
}

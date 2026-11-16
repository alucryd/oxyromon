// SPDX-License-Identifier: GPL-3.0-or-later
//! A Wii U disc or eShop title written as an ORE. A disc is laid out by
//! wiiu's plan, as wux-rs lays out a lossless WUA. Each region, the FST and clusters of a disc-key
//! partition and the game partition's contents, is stored decrypted, by the
//! disc key and by its titles' keys, with what encrypts it back; the free
//! space a partition's mastering fills with the region before it is a
//! repeat; headers, the rest of a region's last sector, and the sectors a
//! repeat gets wrong are kept as they are. A title, a NUS folder, is its
//! files end to end, each content decrypted by its title key when its
//! ticket is there, its ticket kept beside them.

use std::collections::HashMap;
use std::path::Path;

use super::*;
use crate::format::{Layer, LayerMode, MEDIA_CDN_TITLE, Piece, Sidecar, WIIU_SECTOR, Wiiu};
use crate::wiiu::disc::{self as wiiu_disc, SECTOR};
use crate::wiiu::error::Error as WiiuError;
use crate::wiiu::image::Image;
use crate::wiiu::nus;
use crate::wiiu::plan::{self, Kind, Plan, Step};
use crate::wiiu_crypt::{BLOCK as WIIU_BLOCK, Layers};

const _: () = assert!(SECTOR == WIIU_SECTOR);

/// How much of a region is read at a time, before its layers' blocks.
const REGION_WINDOW: u64 = 4 << 20;

impl From<WiiuError> for Error {
    fn from(error: WiiuError) -> Self {
        match error {
            WiiuError::Io(error) => Error::Io(error),
            WiiuError::BadMagic { expected, found } => Error::BadMagic { expected, found },
            WiiuError::Unsupported(what) => Error::Unsupported(what),
            WiiuError::Corrupt(what) => Error::Corrupt(what),
            WiiuError::MissingKey(what) => Error::MissingKey(what),
        }
    }
}

/// Whether `input` is a Wii U disc: a WUD, WUX or ISO, by its magic.
pub(super) fn is_wiiu(input: &Path) -> bool {
    Image::open(input)
        .is_ok_and(|image| image.size() > 3 * SECTOR && wiiu_disc::check_magic(&image).is_ok())
}

/// What a disc or a title reads from: its bytes at a logical offset.
type Source<'a> = &'a dyn Fn(u64, &mut [u8]) -> Result<()>;

/// The disc or title being written: what it reads from, the encoder, its
/// hashes, its areas so far, and how much decrypting and repeats account
/// for.
struct Disc<'a, 'b> {
    read: Source<'a>,
    encoder: Encoder<'b>,
    hashes: Hashes,
    /// A title's file's own hashes, as its DAT lists them.
    file: Hashes,
    areas: Vec<Area>,
    progress: &'a mut dyn FnMut(u64),
    decrypted: u64,
    repeated: u64,
}

fn area(kind: AreaKind, start: u64, len: u64, records: u64) -> Area {
    Area {
        len,
        file: None,
        kind,
        frame: None,
        start,
        records,
        sectors: None,
        title_key: None,
        codec: None,
        exceptions: Vec::new(),
        wiiu: None,
        period: None,
    }
}

impl Disc<'_, '_> {
    /// The disc's bytes from `start`, `len` of them, as they are.
    fn raw(&mut self, start: u64, len: u64) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let records = self.encoder.records;
        let mut segmenter = Segmenter::without_padding(start);
        let mut buf = vec![0; REGION_WINDOW as usize];
        let mut at = 0;
        while at < len {
            let length = (len - at).min(REGION_WINDOW) as usize;
            let buf = &mut buf[..length];
            (self.read)(start + at, buf)?;
            self.hashes.update(buf);
            self.file.update(buf);
            segmenter.feed(buf, at, None, &mut |run| self.encoder.push(run));
            self.encoder.flush(false)?;
            (self.progress)(length as u64);
            at += length as u64;
        }
        segmenter.finish(&mut |run| self.encoder.push(run));
        self.areas.push(area(
            AreaKind::Raw,
            start,
            len,
            self.encoder.records - records,
        ));
        Ok(())
    }

    /// A region from `start`, `len` bytes, encrypted as `wiiu` says: stored
    /// decrypted, each window checked to encrypt back to the disc's bytes.
    fn region(&mut self, start: u64, len: u64, wiiu: Wiiu) -> Result<()> {
        let layers = Layers::new(&wiiu, len)?;
        let records = self.encoder.records;
        let mut segmenter = Segmenter::without_padding(0);
        let mut from = 0;
        while from < len {
            // A window, and around it the inner blocks it needs
            let span = layers.span(len, from, (from + REGION_WINDOW).min(len));
            let mut disc = vec![0; (span.hi - span.lo) as usize];
            (self.read)(start + span.lo, &mut disc)?;
            let mut before = [0; 16];
            if span.lo > 0 {
                (self.read)(start + span.lo - 16, &mut before)?;
            }
            let mut plain = disc.clone();
            layers.decrypt(&span, &before, &mut plain);
            let mut back = plain.clone();
            layers.encrypt(&span, &mut back);
            let window = (span.from - span.lo) as usize..(span.to - span.lo) as usize;
            if back[window.clone()] != disc[window.clone()] {
                return Err(Error::Corrupt(format!(
                    "the Wii U region at {start:#x} does not encrypt back"
                )));
            }
            self.hashes.update(&disc[window.clone()]);
            self.file.update(&disc[window.clone()]);
            segmenter.feed(&plain[window], span.from, None, &mut |run| {
                self.encoder.push(run)
            });
            self.encoder.flush(false)?;
            (self.progress)(span.to - span.from);
            from = span.to;
        }
        segmenter.finish(&mut |run| self.encoder.push(run));
        self.decrypted += len;
        let mut region = area(AreaKind::Wiiu, start, len, self.encoder.records - records);
        region.wiiu = Some(wiiu);
        self.areas.push(region);
        Ok(())
    }

    /// `count` sectors from sector `first`, each the one `period` sectors
    /// before it where it is, kept as it is where it is not.
    fn fill(&mut self, first: u64, count: u64, period: Option<u64>) -> Result<()> {
        let mut sector = vec![0; SECTOR as usize];
        let mut earlier = vec![0; SECTOR as usize];
        // The run being gathered: where it starts, and whether it repeats
        let mut run: Option<(u64, bool)> = None;
        for s in first..first + count {
            (self.read)(s * SECTOR, &mut sector)?;
            let repeats = match period {
                Some(period) if period > 0 && period <= s => {
                    (self.read)((s - period) * SECTOR, &mut earlier)?;
                    earlier == sector
                }
                _ => false,
            };
            if run.is_some_and(|(_, kind)| kind != repeats) {
                self.flush_fill(run.take().unwrap(), s, period)?;
            }
            run.get_or_insert((s, repeats));
            if repeats {
                // A raw run is hashed as it is read again
                self.hashes.update(&sector);
                (self.progress)(SECTOR);
            }
        }
        if let Some(run) = run {
            self.flush_fill(run, first + count, period)?;
        }
        Ok(())
    }

    fn flush_fill(
        &mut self,
        (from, repeats): (u64, bool),
        to: u64,
        period: Option<u64>,
    ) -> Result<()> {
        let (start, len) = (from * SECTOR, (to - from) * SECTOR);
        if !repeats {
            return self.raw(start, len);
        }
        let mut repeat = area(AreaKind::Repeat, start, len, 0);
        repeat.period = period.map(|period| period * SECTOR);
        self.areas.push(repeat);
        self.repeated += len;
        Ok(())
    }
}

/// One AES layer of `key`, its chain's IVs, if it chains, read with `iv_at`
/// from the 16 bytes before each 64 KiB of it but the first.
fn layer(
    mode: LayerMode,
    key: &Key,
    index: u16,
    len: u64,
    iv_at: &mut dyn FnMut(u64, &mut [u8]) -> Result<()>,
) -> Result<Layer> {
    let mut ivs = Vec::new();
    if mode == LayerMode::Chain {
        let mut iv = [0; 16];
        for k in 1..len.div_ceil(WIIU_BLOCK) {
            iv_at(k * WIIU_BLOCK - 16, &mut iv)?;
            ivs.extend_from_slice(&iv);
        }
    }
    Ok(Layer {
        mode,
        key: key.to_vec(),
        index,
        ivs,
    })
}

/// Write the Wii U disc at `input`, a WUD, WUX or ISO, opened with `keys`,
/// to `part`.
pub(super) fn encode(
    input: &Path,
    part: &Path,
    options: &Options,
    keys: &[Key],
    store: Option<&mut StoreWriter>,
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    let Plan {
        image,
        disc_key,
        game,
        header_sectors,
        partitions,
        ..
    } = plan::plan(input, keys)?;
    let size = image.size();
    // Whether each of the game's contents is hashed, by index
    let hashed: HashMap<u16, bool> = nus::parse_tmd(&game.si_file("title.tmd")?)?
        .contents
        .iter()
        .map(|content| (content.index, content.is_hashed()))
        .collect();
    let mut encoder = Encoder::new(part, options, store)?;
    encoder.set_codec(options.codec)?;
    let read = |at: u64, buf: &mut [u8]| Ok(image.read_exact_at(buf, at)?);
    let mut disc = Disc {
        read: &read,
        encoder,
        hashes: Hashes::default(),
        file: Hashes::default(),
        areas: Vec::new(),
        progress,
        decrypted: 0,
        repeated: 0,
    };
    disc.raw(0, header_sectors * SECTOR)?;
    for partition in &partitions {
        let base = partition.start * SECTOR;
        let offsets = plan::stream_offsets(&partition.regions);
        let mut cursor = partition.start;
        for step in partition.steps()? {
            match step {
                Step::Header => {
                    disc.raw(base, partition.header_sectors * SECTOR)?;
                    cursor += partition.header_sectors;
                }
                Step::Fill { count, period } => {
                    disc.fill(cursor, count, period)?;
                    cursor += count;
                }
                Step::Region(i) => {
                    let region = &partition.regions[i];
                    let at = base + region.start * SECTOR;
                    let whole = region.sectors() * SECTOR;
                    let image = &*image;
                    let len = match region.kind {
                        Kind::Plain => {
                            disc.raw(at, whole)?;
                            whole
                        }
                        Kind::Content(index) => {
                            let hashed = hashed.get(&index).copied().unwrap_or(false);
                            // An unhashed content is padded to the AES block
                            let len = match hashed {
                                true => region.len,
                                false => region.len.next_multiple_of(16),
                            }
                            .min(whole);
                            let mode = match hashed {
                                true => LayerMode::Hashed,
                                false => LayerMode::Chain,
                            };
                            let layer =
                                layer(mode, &game.title_key, index, len, &mut |offset, iv| {
                                    Ok(image.read_exact_at(iv, at + offset)?)
                                })?;
                            disc.region(
                                at,
                                len,
                                Wiiu {
                                    outer: None,
                                    pieces: vec![Piece {
                                        offset: 0,
                                        len,
                                        layer,
                                    }],
                                },
                            )?;
                            len
                        }
                        Kind::Raw(index) | Kind::Blocks(index) => {
                            let kind = region.kind;
                            let len = region.len;
                            let outer_mode = match kind {
                                Kind::Raw(_) => LayerMode::Chain,
                                _ => LayerMode::Blocks,
                            };
                            let outer =
                                layer(outer_mode, &disc_key, index, len, &mut |offset, iv| {
                                    Ok(image.read_exact_at(iv, at + offset)?)
                                })?;
                            let stream = offsets[i].expect("a disc-key region is in the stream");
                            let mut pieces = Vec::new();
                            for (offset, piece_len, transform) in
                                plan::pieces(stream, len, &partition.transforms)?
                            {
                                let Some(t) = transform else { continue };
                                let t = &partition.transforms[t];
                                let mode = match t.hashed {
                                    true => LayerMode::Hashed,
                                    false => LayerMode::Chain,
                                };
                                // A piece's chain is in what the disc key
                                // decrypts
                                let layer = layer(
                                    mode,
                                    &partition.keys[&t.ticket],
                                    t.index,
                                    piece_len,
                                    &mut |within, iv| {
                                        Ok(plan::read_plain(
                                            image,
                                            at,
                                            kind,
                                            &disc_key,
                                            offset + within,
                                            iv,
                                        )?)
                                    },
                                )?;
                                pieces.push(Piece {
                                    offset,
                                    len: piece_len,
                                    layer,
                                });
                            }
                            disc.region(
                                at,
                                len,
                                Wiiu {
                                    outer: Some(outer),
                                    pieces,
                                },
                            )?;
                            len
                        }
                    };
                    // The rest of its last sector
                    disc.raw(at + len, whole - len)?;
                    cursor = partition.start + region.start + region.sectors();
                }
            }
        }
        if cursor != partition.end {
            return Err(Error::Corrupt(format!(
                "partition {} is not covered whole",
                partition.name
            )));
        }
    }

    let Disc {
        encoder,
        hashes,
        areas,
        decrypted,
        repeated,
        ..
    } = disc;
    let Hashes {
        crc32,
        md5,
        sha1,
        blake3: logical,
    } = hashes;
    let logical_hash: [u8; 32] = logical.finalize().into();
    let identity = |algo: &str, value: Vec<u8>| Identity {
        algo: algo.into(),
        view: "logical".into(),
        value,
    };
    let meta = Meta {
        media: Media {
            cue: None,
            sbi: None,
            ticket: None,
            size,
            areas,
            class: MEDIA_DVD_2048,
            platform: "wiiu".into(),
        },
        store: None,
        ledger: vec![
            step("wiiu-decrypt", Some(decrypted)),
            step("wiiu-repeat", Some(repeated)),
            step("zero", Some(encoder.zeros)),
            step("mincdc-dedup", Some(encoder.deduped)),
        ],
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
}

/// Whether `input` is a folder holding a Wii U title, a NUS folder: one
/// with its TMD.
pub fn is_title_folder(input: &Path) -> bool {
    input.is_dir() && nus::tmd_path(input).is_some()
}

/// Write the Wii U title in the NUS folder `input`, a game, an update or a
/// DLC, opened with `keys`, to `part`: its files end to end in name order,
/// each content stored decrypted by its title key if the folder holds its
/// ticket, as it is otherwise; the ticket kept beside them, which no DAT
/// lists.
pub(super) fn encode_title(
    input: &Path,
    part: &Path,
    options: &Options,
    keys: &[Key],
    store: Option<&mut StoreWriter>,
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    let tmd_path = nus::tmd_path(input)
        .ok_or_else(|| Error::Unsupported("a folder holding no Wii U title".into()))?;
    let tmd = nus::parse_tmd(&std::fs::read(&tmd_path)?)?;
    let ticket = match nus::ticket_path(input) {
        Some(path) => Some(Sidecar {
            name: file_name(&path),
            bytes: std::fs::read(&path)?,
        }),
        None => None,
    };
    // Its files, the ticket aside, in name order
    let mut names = Vec::new();
    for entry in std::fs::read_dir(input)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_file() && ticket.as_ref().is_none_or(|t| t.name != name) {
            names.push(name);
        }
    }
    names.sort();
    // A content's file, as No-Intro names it, or as downloaders do
    let content = |name: &str| {
        let name = name.to_lowercase();
        tmd.contents
            .iter()
            .find(|c| name == format!("{:08x}", c.id) || name == format!("{:08x}.app", c.id))
    };
    // Its title key, from its ticket, by whichever key opens content 0,
    // the FST
    let title_key = match &ticket {
        None => None,
        Some(ticket) => {
            let (title_id, encrypted) = plan::parse_ticket(&ticket.bytes)?;
            let fst = names
                .iter()
                .find(|name| content(name).is_some_and(|c| c.index == 0))
                .ok_or_else(|| Error::Corrupt("the title has no content 0".into()))?;
            let fst = std::fs::read(input.join(fst))?;
            Some(plan::title_key(&fst, title_id, &encrypted, keys)?)
        }
    };
    let mut files = Vec::with_capacity(names.len());
    let mut size = 0;
    for name in &names {
        let file = File::open(input.join(name))?;
        let len = file.metadata()?.len();
        if len == 0 {
            return Err(Error::Unsupported(format!("an empty file: {name}")));
        }
        files.push((size, len, file));
        size += len;
    }
    // A file's bytes at a logical offset: reads stay within one
    let read = |at: u64, buf: &mut [u8]| -> Result<()> {
        let (start, len, file) = files
            .iter()
            .find(|(start, len, _)| (*start..start + len).contains(&at))
            .ok_or_else(|| Error::Corrupt("a read past the title".into()))?;
        if at + buf.len() as u64 > start + len {
            return Err(Error::Corrupt("a read across a title's files".into()));
        }
        Ok(io::read_exact_at(file, buf, at - start)?)
    };

    let mut encoder = Encoder::new(part, options, store)?;
    encoder.set_codec(options.codec)?;
    let mut disc = Disc {
        read: &read,
        encoder,
        hashes: Hashes::default(),
        file: Hashes::default(),
        areas: Vec::new(),
        progress,
        decrypted: 0,
        repeated: 0,
    };
    let mut identity = Vec::new();
    for (name, (start, len, _)) in names.iter().zip(&files) {
        let (start, len) = (*start, *len);
        let first = disc.areas.len();
        match (content(name), &title_key) {
            (Some(content), Some(key)) if len == content.stored_size() => {
                let mode = match content.is_hashed() {
                    true => LayerMode::Hashed,
                    false => LayerMode::Chain,
                };
                let layer = layer(mode, key, content.index, len, &mut |offset, iv| {
                    read(start + offset, iv)
                })?;
                disc.region(
                    start,
                    len,
                    Wiiu {
                        outer: None,
                        pieces: vec![Piece {
                            offset: 0,
                            len,
                            layer,
                        }],
                    },
                )?;
            }
            _ => disc.raw(start, len)?,
        }
        for area in &mut disc.areas[first..] {
            area.file = Some(name.clone());
        }
        let Hashes {
            crc32, md5, sha1, ..
        } = std::mem::take(&mut disc.file);
        identity.extend(file_identity(name, crc32, md5, sha1));
    }
    if let Some(ticket) = &ticket {
        let mut hashes = Hashes::default();
        hashes.update(&ticket.bytes);
        let Hashes {
            crc32, md5, sha1, ..
        } = hashes;
        identity.extend(file_identity(&ticket.name, crc32, md5, sha1));
    }

    let Disc {
        encoder,
        hashes,
        areas,
        decrypted,
        ..
    } = disc;
    let logical_hash: [u8; 32] = hashes.blake3.finalize().into();
    identity.insert(
        0,
        Identity {
            algo: "blake3-256".into(),
            view: "logical".into(),
            value: logical_hash.to_vec(),
        },
    );
    let meta = Meta {
        media: Media {
            cue: None,
            sbi: None,
            ticket,
            size,
            areas,
            class: MEDIA_CDN_TITLE,
            platform: "wiiu".into(),
        },
        store: None,
        ledger: vec![
            step("wiiu-decrypt", Some(decrypted)),
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
}

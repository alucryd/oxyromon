//! A WUA back to the disc it was made from, as a WUD or a WUX, byte for byte,
//! from what its `.wud/` folder keeps: see [`crate::lossless`].

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use sha1::{Digest, Sha1};
use sha2::Sha256;

use crate::Stats;
use crate::crypto::Key;
use crate::disc::{self, SECTOR};
use crate::error::{Error, Result, corrupt};
use crate::io::{finish, part, read_exact_at};
use crate::lossless::{
    DISC_DIR, ENTRY_WINDOW_LOG, Kind, Layout, Region, Step, Storage, Transform, common_by_sha1,
    parse_transforms, pieces, read_entry_head, store_name, ticket_key, title_crypt,
};
use crate::revert::{Contents, REVERT_DIR, encrypt};
use crate::wux::{HEADER_SIZE, Header, SECTOR_SIZE};
use crate::zarchive::{NodeId, ROOT, Reader};

const BLOCK: u64 = 0x10000;
const CHUNK: u64 = 16 * BLOCK;

/// Rebuild the disc the WUA at `input` was made from as the WUX at `output`,
/// written as wudd writes it, each distinct sector once, and checked against
/// the disc's SHA-1.
///
/// Only WUAs written with what rebuilds their disc can be. `store` is the
/// folder holding update partitions, if the WUA's is there; `keys` must hold
/// the disc's key and the Wii U common key. `progress` is called with input
/// bytes, scaled from the disc's sectors written.
pub fn wua_to_wux(
    input: &Path,
    output: &Path,
    store: Option<&Path>,
    keys: &[Key],
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    let part = part(output);
    let result = (|| {
        let layout = read_layout(input)?;
        let mut sink = WuxSink::new(create(&part)?, layout.info.size)?;
        rebuild(input, &layout, store, keys, &mut sink, progress)?;
        let output_size = sink.finish()?;
        Ok(Stats {
            input_size: std::fs::metadata(input)?.len(),
            output_size,
        })
    })();
    finish(&part, output, result)
}

/// Rebuild the disc the WUA at `input` was made from as the WUD at `output`;
/// see [`wua_to_wux`].
pub fn wua_to_wud(
    input: &Path,
    output: &Path,
    store: Option<&Path>,
    keys: &[Key],
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    let part = part(output);
    let result = (|| {
        let layout = read_layout(input)?;
        let mut sink = WudSink {
            file: BufWriter::with_capacity(1 << 20, create(&part)?),
            dirty: false,
            sha1: Sha1::new(),
            buf: vec![0; SECTOR as usize],
        };
        rebuild(input, &layout, store, keys, &mut sink, progress)?;
        sink.file.flush()?;
        sink.file.get_ref().sync_all()?;
        Ok(Stats {
            input_size: std::fs::metadata(input)?.len(),
            output_size: layout.info.size,
        })
    })();
    finish(&part, output, result)
}

/// A new file, read back as it is written.
fn create(path: &Path) -> Result<File> {
    Ok(std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?)
}

fn read_layout(input: &Path) -> Result<Layout> {
    let mut reader = Reader::open(input)?;
    let node = reader
        .lookup(ROOT, &format!("{DISC_DIR}/layout"))
        .ok_or_else(|| {
            Error::Unsupported(
                "a WUA without what rebuilds its disc, which only WUAs written with it hold".into(),
            )
        })?;
    Layout::parse(&reader.read_file(node)?)
}

/// Where the disc goes, a sector at a time, in order, each hashed as it goes.
trait Sink {
    fn sector(&mut self, bytes: &[u8]) -> Result<()>;
    /// The next sector is the same as sector `from`.
    fn copy(&mut self, from: u64) -> Result<()>;
    fn sha1(&mut self) -> [u8; 20];
}

struct WudSink {
    file: BufWriter<File>,
    dirty: bool,
    sha1: Sha1,
    buf: Vec<u8>,
}

impl Sink for WudSink {
    fn sector(&mut self, bytes: &[u8]) -> Result<()> {
        self.file.write_all(bytes)?;
        self.sha1.update(bytes);
        self.dirty = true;
        Ok(())
    }

    fn copy(&mut self, from: u64) -> Result<()> {
        if self.dirty {
            self.file.flush()?;
            self.dirty = false;
        }
        read_exact_at(self.file.get_ref(), &mut self.buf, from * SECTOR)?;
        self.file.write_all(&self.buf)?;
        self.sha1.update(&self.buf);
        self.dirty = true;
        Ok(())
    }

    fn sha1(&mut self) -> [u8; 20] {
        std::mem::take(&mut self.sha1).finalize().into()
    }
}

/// A WUX, as [`crate::compress`] writes it: distinct sectors in the order
/// first used, told apart by SHA-256. A copied sector is the same as its
/// source, so it is never new.
struct WuxSink {
    file: BufWriter<File>,
    header: Header,
    table: Vec<u32>,
    ids: HashMap<[u8; 32], u32>,
    dirty: bool,
    sha1: Sha1,
    buf: Vec<u8>,
}

impl WuxSink {
    fn new(file: File, size: u64) -> Result<WuxSink> {
        let header = Header {
            sector_size: SECTOR_SIZE,
            uncompressed_size: size,
            flags: 0,
        };
        let mut file = BufWriter::with_capacity(1 << 20, file);
        file.write_all(&header.to_bytes())?;
        std::io::copy(
            &mut std::io::Read::take(std::io::repeat(0), header.data_offset() - HEADER_SIZE),
            &mut file,
        )?;
        Ok(WuxSink {
            file,
            header,
            table: Vec::with_capacity(header.sector_count() as usize),
            ids: HashMap::new(),
            dirty: false,
            sha1: Sha1::new(),
            buf: vec![0; SECTOR as usize],
        })
    }

    fn finish(mut self) -> Result<u64> {
        let mut file = self.file.into_inner().map_err(|e| e.into_error())?;
        let bytes: Vec<u8> = self.table.iter().flat_map(|i| i.to_le_bytes()).collect();
        file.seek(SeekFrom::Start(HEADER_SIZE))?;
        file.write_all(&bytes)?;
        let size = file.seek(SeekFrom::End(0))?;
        file.sync_all()?;
        let _ = &mut self.buf;
        Ok(size)
    }
}

impl Sink for WuxSink {
    fn sector(&mut self, bytes: &[u8]) -> Result<()> {
        let next = self.ids.len() as u32;
        let id = *self.ids.entry(Sha256::digest(bytes).into()).or_insert(next);
        if id == next {
            self.file.write_all(bytes)?;
            self.dirty = true;
        }
        self.table.push(id);
        self.sha1.update(bytes);
        Ok(())
    }

    fn copy(&mut self, from: u64) -> Result<()> {
        let id = self.table[from as usize];
        self.table.push(id);
        if self.dirty {
            self.file.flush()?;
            self.dirty = false;
        }
        read_exact_at(
            self.file.get_ref(),
            &mut self.buf,
            self.header.data_offset() + u64::from(id) * SECTOR,
        )?;
        self.sha1.update(&self.buf);
        Ok(())
    }

    fn sha1(&mut self) -> [u8; 20] {
        std::mem::take(&mut self.sha1).finalize().into()
    }
}

/// The disc's sectors, in order, with those kept whole put in their place,
/// and progress reported.
struct Disc<'a> {
    sink: &'a mut dyn Sink,
    next: u64,
    total: u64,
    patches: HashMap<u64, usize>,
    patch_bytes: Vec<u8>,
    /// A sector being filled.
    pending: Vec<u8>,
    progress: &'a mut dyn FnMut(u64),
    input_size: u64,
    reported: u64,
}

impl Disc<'_> {
    fn emit(&mut self, bytes: &[u8]) -> Result<()> {
        match self.patches.get(&self.next) {
            Some(&at) => {
                let patch = &self.patch_bytes[at..at + SECTOR as usize];
                self.sink.sector(patch)?;
            }
            None => self.sink.sector(bytes)?,
        }
        self.advance()
    }

    fn copy(&mut self, from: Option<u64>) -> Result<()> {
        match (self.patches.get(&self.next), from) {
            (Some(&at), _) => {
                let patch = self.patch_bytes[at..at + SECTOR as usize].to_vec();
                self.sink.sector(&patch)?;
            }
            (None, Some(from)) => self.sink.copy(from)?,
            (None, None) => {
                return Err(corrupt(format!(
                    "sector {} of the disc has nothing to rebuild it from",
                    self.next
                )));
            }
        }
        self.advance()
    }

    fn advance(&mut self) -> Result<()> {
        self.next += 1;
        if self.next > self.total {
            return Err(corrupt("the disc layout runs past the disc's end"));
        }
        let scaled = (u128::from(self.next) * u128::from(self.input_size)
            / u128::from(self.total.max(1))) as u64;
        (self.progress)(scaled - self.reported);
        self.reported = scaled;
        Ok(())
    }

    /// Append bytes of a region, a sector at a time.
    fn push(&mut self, mut bytes: &[u8]) -> Result<()> {
        while !bytes.is_empty() {
            if self.pending.is_empty() && bytes.len() >= SECTOR as usize {
                let (sector, rest) = bytes.split_at(SECTOR as usize);
                self.emit(sector)?;
                bytes = rest;
                continue;
            }
            let take = (SECTOR as usize - self.pending.len()).min(bytes.len());
            self.pending.extend_from_slice(&bytes[..take]);
            bytes = &bytes[take..];
            if self.pending.len() == SECTOR as usize {
                let sector = std::mem::take(&mut self.pending);
                self.emit(&sector)?;
                self.pending = sector;
                self.pending.clear();
            }
        }
        Ok(())
    }

    /// End a region: its last sector is zero past its end.
    fn end_region(&mut self) -> Result<()> {
        if !self.pending.is_empty() {
            let mut sector = std::mem::take(&mut self.pending);
            sector.resize(SECTOR as usize, 0);
            self.emit(&sector)?;
            sector.clear();
            self.pending = sector;
        }
        Ok(())
    }
}

/// A partition's stream: in the WUA, or in a store entry, read through.
struct Stream {
    data: Data,
    len: u64,
    transforms: Vec<Transform>,
    keys: HashMap<(u64, u32), Key>,
}

enum Data {
    Archive {
        reader: Reader,
        base: u64,
    },
    Entry {
        decoder: zstd::Decoder<'static, BufReader<File>>,
        at: u64,
    },
}

impl Data {
    fn read(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        match self {
            Data::Archive { reader, base } => reader.read(*base + offset, buf),
            Data::Entry { decoder, at } => {
                if offset != *at {
                    return Err(corrupt("a store entry read out of order"));
                }
                decoder.read_exact(buf)?;
                *at += buf.len() as u64;
                Ok(())
            }
        }
    }
}

impl Stream {
    /// The stream in the WUA's folder `dir`, with its transforms beside it.
    fn in_archive(
        mut reader: Reader,
        dir: NodeId,
        name: &str,
        len: u64,
        candidates: &[Key],
    ) -> Result<Stream> {
        let missing = |what: &str| corrupt(format!("the stream of {name} has no {what}"));
        let stream = reader
            .lookup(dir, "stream")
            .ok_or_else(|| missing("data"))?;
        let (base, size) = reader.file(stream).ok_or_else(|| missing("data"))?;
        if size != len {
            return Err(corrupt(format!("the stream of {name} is not its length")));
        }
        let transforms = reader
            .lookup(dir, "transforms")
            .ok_or_else(|| missing("transforms"))?;
        let transforms = parse_transforms(&reader.read_file(transforms)?)?;
        let mut keys = HashMap::new();
        for t in &transforms {
            if keys.contains_key(&t.ticket) {
                continue;
            }
            if t.ticket.0 + u64::from(t.ticket.1) > len {
                return Err(corrupt(format!("a ticket past the stream of {name}")));
            }
            let mut ticket = vec![0; t.ticket.1 as usize];
            reader.read(base + t.ticket.0, &mut ticket)?;
            let common = common_by_sha1(candidates, &t.common)?;
            keys.insert(t.ticket, ticket_key(&ticket, &common)?);
        }
        Ok(Stream {
            data: Data::Archive { reader, base },
            len,
            transforms,
            keys,
        })
    }

    /// The stream of the store entry at `path`.
    fn in_entry(path: &Path, len: u64, candidates: &[Key]) -> Result<Stream> {
        let mut decoder = zstd::Decoder::new(File::open(path)?)?;
        decoder.window_log_max(ENTRY_WINDOW_LOG)?;
        let (transforms, tickets) = read_entry_head(&mut decoder)?;
        let mut keys = HashMap::new();
        for t in &transforms {
            if keys.contains_key(&t.ticket) {
                continue;
            }
            let ticket = tickets.get(&t.ticket).ok_or_else(|| {
                corrupt(format!(
                    "the store entry lacks the ticket of the content at {}",
                    t.offset
                ))
            })?;
            let common = common_by_sha1(candidates, &t.common)?;
            keys.insert(t.ticket, ticket_key(ticket, &common)?);
        }
        Ok(Stream {
            data: Data::Entry { decoder, at: 0 },
            len,
            transforms,
            keys,
        })
    }
}

/// Encrypt a region with the disc key as it is on the disc, carrying the
/// chain from one piece to the next.
struct Encoder {
    kind: Kind,
    key: Key,
    at: u64,
    iv: [u8; 16],
}

impl Encoder {
    fn push(&mut self, buf: &mut [u8]) {
        match self.kind {
            Kind::Content(_) | Kind::Plain => {}
            Kind::Raw(index) => {
                if self.at == 0 {
                    self.iv = disc::block_iv(index, 0);
                }
                encrypt(&self.key, &mut self.iv, buf);
            }
            Kind::Blocks(index) => {
                let mut done = 0;
                while done < buf.len() {
                    let at = self.at + done as u64;
                    if at.is_multiple_of(BLOCK) {
                        self.iv = disc::block_iv(index, at / BLOCK);
                    }
                    let take = ((BLOCK - at % BLOCK) as usize).min(buf.len() - done);
                    encrypt(&self.key, &mut self.iv, &mut buf[done..done + take]);
                    done += take;
                }
            }
        }
        self.at += buf.len() as u64;
    }
}

fn rebuild(
    input: &Path,
    layout: &Layout,
    store: Option<&Path>,
    keys: &[Key],
    sink: &mut dyn Sink,
    progress: &mut dyn FnMut(u64),
) -> Result<()> {
    let mut reader = Reader::open(input)?;
    let dir = reader
        .lookup(ROOT, DISC_DIR)
        .ok_or_else(|| corrupt("the WUA has no disc folder"))?;
    let file = |reader: &mut Reader, name: &str| -> Result<Vec<u8>> {
        let node = reader
            .lookup(dir, name)
            .ok_or_else(|| corrupt(format!("the WUA's disc folder has no {name}")))?;
        reader.read_file(node)
    };
    let header = file(&mut reader, "header")?;
    let patch_bytes = file(&mut reader, "patches")?;
    if patch_bytes.len() as u64 != layout.patches.len() as u64 * SECTOR {
        return Err(corrupt("the disc's kept sectors are not whole"));
    }
    let header_len: u64 = layout.header_sectors
        + layout
            .partitions
            .iter()
            .map(|p| p.header_sectors)
            .sum::<u64>();
    if header.len() as u64 != header_len * SECTOR || layout.header_sectors < 4 {
        return Err(corrupt(
            "the disc's headers are not as long as its layout says",
        ));
    }
    let disc_key = disc::disc_key_of(
        header[3 * SECTOR as usize + 0x100..][..48]
            .try_into()
            .unwrap(),
        keys,
    )?;

    // The game, from its revert data.
    let revert = reader
        .lookup(ROOT, &format!("{REVERT_DIR}/{}", layout.game))
        .ok_or_else(|| corrupt(format!("the WUA has no revert data for {}", layout.game)))?;
    let tmd = reader
        .lookup(revert, "title.tmd")
        .ok_or_else(|| corrupt(format!("the revert data of {} has no TMD", layout.game)))?;
    let tmd = reader.read_file(tmd)?;
    let mut game_reader = Reader::open(input)?;
    let game = Contents::open(&mut game_reader, &layout.game, revert, &tmd, keys)?;

    let mut disc = Disc {
        sink,
        next: 0,
        total: layout.info.size / SECTOR,
        patches: layout
            .patches
            .iter()
            .enumerate()
            .map(|(i, &s)| (s, i * SECTOR as usize))
            .collect(),
        patch_bytes,
        pending: Vec::with_capacity(SECTOR as usize),
        progress,
        input_size: std::fs::metadata(input)?.len(),
        reported: 0,
    };
    if !layout.info.size.is_multiple_of(SECTOR) {
        return Err(corrupt("the disc is not whole sectors"));
    }
    let mut headers = header.chunks(SECTOR as usize);
    for _ in 0..layout.header_sectors {
        disc.emit(headers.next().unwrap())?;
    }
    let mut buf = vec![0; CHUNK as usize];
    for (i, p) in layout.partitions.iter().enumerate() {
        if disc.next != p.start {
            return Err(corrupt(format!(
                "partition {} is not where it should be",
                p.name
            )));
        }
        let mut stream = match p.storage {
            Storage::None => None,
            Storage::Embedded => {
                let reader = Reader::open(input)?;
                let node = reader
                    .lookup(dir, &format!("{i:02}"))
                    .ok_or_else(|| corrupt(format!("the WUA has no stream for {}", p.name)))?;
                Some(Stream::in_archive(
                    reader,
                    node,
                    &p.name,
                    p.stream_len,
                    keys,
                )?)
            }
            Storage::Store => {
                let name = store_name(&p.stream_sha1);
                let path = store
                    .map(|store| store.join(&name))
                    .filter(|path| path.is_file());
                let Some(path) = path else {
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("update partition {} is not in the store, as {name}", p.name),
                    )));
                };
                Some(Stream::in_entry(&path, p.stream_len, keys)?)
            }
        };
        let offsets = p.stream_offsets();
        let mut stream_sha1 = Sha1::new();
        let mut written = 0u64;
        for step in p.steps()? {
            match step {
                Step::Header => {
                    for _ in 0..p.header_sectors {
                        disc.emit(headers.next().unwrap())?;
                    }
                }
                Step::Fill { count, period } => {
                    for _ in 0..count {
                        let from = period.map(|period| disc.next - period);
                        disc.copy(from)?;
                    }
                }
                Step::Region(r) => {
                    let region = &p.regions[r];
                    if disc.next != p.start + region.start {
                        return Err(corrupt(format!("a region of {} is misplaced", p.name)));
                    }
                    match region.kind {
                        Kind::Content(index) => {
                            let content = game
                                .tmd
                                .contents
                                .iter()
                                .find(|c| c.index == index)
                                .ok_or_else(|| {
                                    corrupt(format!("the game has no content {index}"))
                                })?;
                            if content.size != region.len {
                                return Err(corrupt(format!(
                                    "content {index} is not the size of its region"
                                )));
                            }
                            game.write(
                                &mut game_reader,
                                content,
                                &mut |chunk| disc.push(chunk),
                                &mut |_| {},
                            )?;
                        }
                        kind => {
                            let stream = stream.as_mut().ok_or_else(|| {
                                corrupt(format!("partition {} has regions but no stream", p.name))
                            })?;
                            let offset = offsets[r].unwrap();
                            written += region.len;
                            write_region(
                                stream,
                                region,
                                offset,
                                kind,
                                &disc_key,
                                &mut buf,
                                &mut stream_sha1,
                                &mut disc,
                            )?;
                        }
                    }
                    disc.end_region()?;
                }
            }
        }
        if let Some(stream) = &stream {
            if written != stream.len {
                return Err(corrupt(format!("the stream of {} is left over", p.name)));
            }
            if stream_sha1.finalize().as_slice() != p.stream_sha1 {
                return Err(corrupt(format!(
                    "the stream of {} does not check against its SHA-1",
                    p.name
                )));
            }
        }
    }
    if disc.next != disc.total {
        return Err(corrupt("the disc layout stops short of the disc's end"));
    }
    if disc.sink.sha1() != layout.info.sha1 {
        return Err(corrupt("the rebuilt disc does not check against its SHA-1"));
    }
    Ok(())
}

/// A region from the stream: its titles' contents encrypted again with their
/// title keys, hashed as the stream was, then encrypted with the disc key.
#[allow(clippy::too_many_arguments)]
fn write_region(
    stream: &mut Stream,
    region: &Region,
    offset: u64,
    kind: Kind,
    disc_key: &Key,
    buf: &mut [u8],
    sha1: &mut Sha1,
    disc: &mut Disc,
) -> Result<()> {
    let mut encoder = Encoder {
        kind,
        key: *disc_key,
        at: 0,
        iv: [0; 16],
    };
    for (at, len, t) in pieces(offset, region.len, &stream.transforms)? {
        let mut iv = [0; 16];
        let mut done = 0;
        while done < len {
            let n = (len - done).min(CHUNK) as usize;
            let chunk = &mut buf[..n];
            stream.data.read(offset + at + done, chunk)?;
            if let Some(t) = t {
                let t = &stream.transforms[t];
                title_crypt(t, &stream.keys[&t.ticket], done, chunk, &mut iv, false);
            }
            sha1.update(&*chunk);
            encoder.push(chunk);
            disc.push(chunk)?;
            done += n as u64;
        }
    }
    Ok(())
}

//! Titles to WUA, the way Cemu converts them: each title's files, decrypted,
//! in a `<title id>_v<version>` folder of a ZArchive. A title comes from a
//! disc, a WUD or WUX, or from a NUS folder, as updates and DLC do.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::rc::Rc;

use crate::Stats;
use crate::crypto::{Key, decrypt_block};
use crate::disc::{self, Fst, PartitionSource, Source};
use crate::error::{Error, Result, corrupt};
use crate::image::Image;
use crate::io::{Scaled, finish, part};
use crate::nus::{self, NusSource, Tmd};
use crate::revert::{self, Meta, REVERT_DIR};
use crate::zarchive::{self, Writer};

/// Convert `inputs` to one WUA at `output`, a folder per title. Each input is
/// a WUD or WUX, or a NUS folder holding a title's TMD, ticket and `.app`s:
/// a game, its update and its DLC make one WUA, as Cemu makes them.
///
/// `keys` are candidates, as in Cemu's `keys.txt`: among them must be each
/// disc's key and the Wii U common key, which this crate does not ship.
/// `progress` is called with input bytes, scaled from the bytes read.
///
/// Like Cemu, only a disc's first game (GM) partition is converted, but the
/// WUA keeps what rebuilds the whole disc, byte for byte: see
/// [`crate::wua_to_wux`]. Its update partition, the same on every disc that
/// carries that update, goes to `store` when given, where it is written
/// once, and into the WUA otherwise. A WUA holds at most one disc.
// ponytail: first GM partition only, as Cemu does; loop over every GM
// partition, each with its own ticket in the SI partition, if discs bundling
// updates or DLC turn up.
pub fn to_wua(
    inputs: &[&Path],
    output: &Path,
    keys: &[Key],
    store: Option<&Path>,
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    to_wua_with_tickets(inputs, output, keys, &[], store, progress)
}

/// [`to_wua`], with tickets for the NUS folders that hold none: `tickets`
/// are candidates, as `keys` are, each found by its title ID. Nintendo's
/// servers deliver no ticket for DLC, so No-Intro lists none, and a
/// downloader's is kept apart from the title's files.
pub fn to_wua_with_tickets(
    inputs: &[&Path],
    output: &Path,
    keys: &[Key],
    tickets: &[Vec<u8>],
    store: Option<&Path>,
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    let mut titles = Vec::with_capacity(inputs.len());
    for input in inputs {
        let title = match input.is_dir() {
            true => open_nus(input, keys, tickets)?,
            false => open_disc(input, keys)?,
        };
        if titles.iter().any(|t: &Title| t.title_id == title.title_id) {
            return Err(Error::Unsupported(format!(
                "{:016x} is among the inputs twice",
                title.title_id
            )));
        }
        titles.push(title);
    }
    let discs: Vec<&Title> = titles.iter().filter(|t| t.disc).collect();
    if discs.len() > 1 {
        return Err(Error::Unsupported("a WUA of more than one disc".into()));
    }
    let disc = discs.first().map(|t| t.folder());
    let disc_input = inputs.iter().find(|input| !input.is_dir());
    let input_size: u64 = titles.iter().map(|t| t.input_size).sum();
    let trees: Vec<Vec<Item>> = titles.iter().map(|t| tree(&t.fst)).collect();
    // The files written, then every content read for the revert data.
    let mut total: u64 = titles
        .iter()
        .zip(&trees)
        .flat_map(|(t, tree)| {
            tree.iter()
                .filter(|item| !item.is_dir)
                .map(|item| t.fst.entries[item.entry].size())
        })
        .sum();
    for title in &titles {
        total += nus::parse_tmd(&title.meta.tmd)?
            .contents
            .iter()
            .map(|c| c.size)
            .sum::<u64>();
    }
    // The disc read through, then its partitions' streams.
    if let Some(disc_input) = disc_input {
        total += 2 * Image::open(disc_input)?.size();
    }

    let part = part(output);
    let result = (|| {
        let mut writer = Writer::new(BufWriter::with_capacity(1 << 20, File::create(&part)?));
        let mut scaled = Scaled::new(progress, total, input_size);
        for (title, tree) in titles.iter().zip(&trees) {
            let root = writer.dir(zarchive::ROOT, title.folder().as_bytes())?;
            // Archive nodes of the FST's directories, by FST index.
            let mut dirs = vec![root; title.fst.entries.len()];
            for item in tree {
                let parent = dirs[item.parent];
                let name = title.fst.name(item.entry)?;
                if item.is_dir {
                    dirs[item.entry] = writer.dir(parent, name)?;
                    continue;
                }
                writer.file(parent, name)?;
                title
                    .fst
                    .read_with(title.source.as_ref(), item.entry, &mut |chunk| {
                        writer.append(chunk)?;
                        scaled.add(chunk.len() as u64);
                        Ok(())
                    })?;
            }
        }
        // What rebuilds each title's contents from its files, ignored by Cemu.
        let revert = writer.dir(zarchive::ROOT, REVERT_DIR.as_bytes())?;
        for (title, tree) in titles.iter().zip(&trees) {
            let files = revert::collect(
                title.source.as_ref(),
                &title.fst,
                tree,
                &title.meta,
                &mut |n| scaled.add(n),
            )?;
            let dir = writer.dir(revert, title.folder().as_bytes())?;
            for (name, bytes) in files {
                writer.file(dir, name.as_bytes())?;
                writer.append(&bytes)?;
            }
        }
        if let (Some(folder), Some(disc_input)) = (&disc, disc_input) {
            crate::lossless::write(&mut writer, disc_input, folder, keys, store, &mut |n| {
                scaled.add(n)
            })?;
        }
        scaled.finish();
        let file = writer.finish()?.into_inner().map_err(|e| e.into_error())?;
        file.sync_all()?;
        Ok(Stats {
            input_size,
            output_size: file.metadata()?.len(),
        })
    })();
    finish(&part, output, result)
}

/// A title to convert: where its clusters are, its FST, its WUA folder, and
/// the size of what it was read from.
struct Title {
    source: Box<dyn Source>,
    fst: Fst,
    meta: Meta,
    title_id: u64,
    version: u16,
    input_size: u64,
    /// Whether it comes from a disc.
    disc: bool,
}

impl Title {
    /// Its WUA folder, named as Cemu names it.
    fn folder(&self) -> String {
        format!("{:016x}_v{}", self.title_id, self.version)
    }
}

/// The title ID and version of `input`: a NUS folder's from its TMD, needing
/// no key, a disc's from its game's code/app.xml, needing its keys. A game
/// and its update and DLC share the low half of their title IDs.
pub fn title_id(input: &Path, keys: &[Key]) -> Result<(u64, u16)> {
    if input.is_dir() {
        let tmd_path = nus::tmd_path(input).ok_or_else(|| corrupt("the title has no TMD"))?;
        let tmd = nus::parse_tmd(&std::fs::read(tmd_path)?)?;
        return Ok((tmd.title_id, tmd.version));
    }
    let title = open_disc(input, keys)?;
    Ok((title.title_id, title.version))
}

/// A disc's game partition, opened: its SI partition, which holds its TMD,
/// ticket and certificates, and its FST, with the title key its files
/// decrypt with. Like Cemu, the first game (GM) partition.
pub(crate) struct DiscGame {
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

fn si_file(fst: &Fst, source: &PartitionSource, index: usize, name: &str) -> Result<Vec<u8>> {
    let path = format!("{index:02x}/{name}");
    let file = fst
        .find(&path)
        .ok_or_else(|| corrupt(format!("the SI partition has no {path}")))?;
    fst.read(source, file)
}

/// A disc's game, named after its code/app.xml, as Cemu names it.
fn open_disc(input: &Path, keys: &[Key]) -> Result<Title> {
    let game = DiscGame::open(input, keys)?;
    let app = game
        .fst
        .find("code/app.xml")
        .ok_or_else(|| corrupt("the game partition has no code/app.xml"))?;
    let app = String::from_utf8_lossy(&game.fst.read(&game.source, app)?).into_owned();
    let title_id = xml_hex(&app, "title_id")?;
    let version = xml_hex(&app, "title_version")?;
    let meta = Meta {
        tmd: game.si_file("title.tmd")?,
        ticket: game.si_file("title.tik")?,
        cert: Some(game.si_file("title.cert")?),
        fst_start: game.fst_start,
    };
    Ok(Title {
        source: Box::new(game.source),
        fst: game.fst,
        meta,
        title_id,
        version: version as u16,
        input_size: std::fs::metadata(input)?.len(),
        disc: true,
    })
}

/// A NUS title, named after its TMD.
fn open_nus(dir: &Path, keys: &[Key], tickets: &[Vec<u8>]) -> Result<Title> {
    let NusTitle {
        tmd_bytes,
        tmd,
        ticket,
        source,
        fst,
        fst_size,
        title_key,
    } = open_nus_title(dir, keys, tickets)?;
    let title_id = tmd.title_id;
    let fst = Fst::open(&format!("title {title_id:016x}"), fst, fst_size, title_key)?;
    let mut input_size = 0;
    for entry in std::fs::read_dir(dir)? {
        let metadata = entry?.metadata()?;
        if metadata.is_file() {
            input_size += metadata.len();
        }
    }
    let cert_path = dir.join("title.cert");
    let meta = Meta {
        tmd: tmd_bytes,
        ticket,
        cert: cert_path
            .is_file()
            .then(|| std::fs::read(&cert_path))
            .transpose()?,
        fst_start: 0,
    };
    Ok(Title {
        source: Box::new(source),
        fst,
        meta,
        title_id,
        version: tmd.version,
        input_size,
        disc: false,
    })
}

/// A NUS folder opened: its TMD, its ticket, its contents, and the title key,
/// found with the encrypted FST it decrypts. Its ticket is its own, or else
/// the one of `tickets` for its title.
pub(crate) struct NusTitle {
    pub tmd_bytes: Vec<u8>,
    pub tmd: Tmd,
    pub ticket: Vec<u8>,
    pub source: NusSource,
    pub fst: Vec<u8>,
    pub fst_size: u32,
    pub title_key: Key,
}

pub(crate) fn open_nus_title(dir: &Path, keys: &[Key], tickets: &[Vec<u8>]) -> Result<NusTitle> {
    let tmd_path = nus::tmd_path(dir).ok_or_else(|| corrupt("the title has no TMD"))?;
    let tmd_bytes = std::fs::read(tmd_path)?;
    let tmd = nus::parse_tmd(&tmd_bytes)?;
    let ticket = match nus::ticket_path(dir) {
        Some(path) => std::fs::read(path)?,
        None => tickets
            .iter()
            .find(|ticket| ticket_title_id(ticket).is_ok_and(|id| id == tmd.title_id))
            .cloned()
            .ok_or_else(|| {
                Error::MissingKey("the title has no ticket to decrypt it with".into())
            })?,
    };
    let (title_id, encrypted_key) = parse_ticket(&ticket)?;
    if title_id != tmd.title_id {
        return Err(corrupt(format!(
            "the ticket is for {title_id:016x}, the TMD for {:016x}",
            tmd.title_id
        )));
    }
    let source = NusSource::open(dir, &tmd)?;
    let size = tmd
        .contents
        .iter()
        .find(|c| c.index == 0)
        .ok_or_else(|| corrupt("the TMD lists no FST"))?
        .size;
    let fst_size = u32::try_from(size).map_err(|_| corrupt(format!("the FST is {size} bytes")))?;
    let mut fst = vec![0; (fst_size as usize).next_multiple_of(16)];
    source.read_cluster(0, 0, 0, &mut fst)?;
    let title_key = title_key(&fst, title_id, &encrypted_key, keys)?;
    Ok(NusTitle {
        tmd_bytes,
        tmd,
        ticket,
        source,
        fst,
        fst_size,
        title_key,
    })
}

/// The title ID a ticket is for.
pub fn ticket_title_id(ticket: &[u8]) -> Result<u64> {
    Ok(parse_ticket(ticket)?.0)
}

/// The title ID and encrypted title key in a ticket.
pub(crate) fn parse_ticket(ticket: &[u8]) -> Result<(u64, [u8; 16])> {
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
pub(crate) fn title_key(
    fst: &[u8],
    title_id: u64,
    encrypted_key: &[u8; 16],
    keys: &[Key],
) -> Result<Key> {
    let common = common_key(fst, title_id, encrypted_key, keys)?;
    Ok(decrypt_block(&common, &ticket_iv(title_id), encrypted_key))
}

/// Which of `keys` is the Wii U common key: the one whose title key, from
/// the ticket, decrypts the title's FST.
pub(crate) fn common_key(
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
pub(crate) fn ticket_iv(title_id: u64) -> [u8; 16] {
    let mut iv = [0; 16];
    iv[..8].copy_from_slice(&title_id.to_be_bytes());
    iv
}

/// The hex value of `<tag ...>value</tag>` in an app.xml.
fn xml_hex(xml: &str, tag: &str) -> Result<u64> {
    let missing = || corrupt(format!("code/app.xml has no {tag}"));
    let start = xml.find(&format!("<{tag}")).ok_or_else(missing)?;
    let rest = &xml[start..];
    let value = &rest[rest.find('>').ok_or_else(missing)? + 1..];
    let value = value[..value.find('<').ok_or_else(missing)?].trim();
    u64::from_str_radix(value, 16).map_err(|_| corrupt(format!("code/app.xml's {tag} is {value}")))
}

/// An FST entry to write, under the directory at FST index `parent` (0 for
/// the root).
pub(crate) struct Item {
    pub entry: usize,
    pub parent: usize,
    pub is_dir: bool,
}

/// The FST's entries in order, links and what's under them left out.
pub(crate) fn tree(fst: &Fst) -> Vec<Item> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_xml_values_are_hex() {
        let xml = "<app>\n  <title_version type=\"hexBinary\" length=\"2\">0010</title_version>\n  \
                   <title_id type=\"hexBinary\" length=\"8\">0005000010180700</title_id>\n</app>";
        assert_eq!(xml_hex(xml, "title_id").unwrap(), 0x0005000010180700);
        assert_eq!(xml_hex(xml, "title_version").unwrap(), 16);
        assert!(xml_hex(xml, "group_id").is_err());
    }
}

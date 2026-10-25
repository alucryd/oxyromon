//! Revert data: what a WUA holds, besides its titles' decrypted files, to
//! rebuild each title as a NUS folder whose contents are byte for byte those
//! it came from, checked against its TMD.
//!
//! Per title, under `.nus/<title id>_v<version>/` (Cemu reads only the root's
//! `<title id>_v<version>` folders, so it ignores `.nus`): its `title.tmd`,
//! `title.tik` and, when known, `title.cert`; content 0, the encrypted FST, as
//! `<content id>.app`; and `residue`, every byte of a content that is neither
//! in a file nor rebuilt. Raw contents are zeros between files; hashed ones
//! are too, but for 32 bytes each, and their hash areas are recomputed from
//! their data. Any hash area that is not is kept whole.
//!
//! `residue` is a list of records, all big-endian: a kind (0 for bytes of a
//! content's data, 1 for a block's hash area), the content's index, the data
//! offset or block number, a length, and the bytes.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use sha1::{Digest, Sha1};
use sha2::Sha256;

use crate::Stats;
use crate::crypto::{Key, cbc_decrypt};
use crate::disc::{Fst, Source};
use crate::error::{Error, Result, corrupt};
use crate::nus::{self, Content, HASHED, SHA1};
use crate::wua::{Item, parse_ticket, title_key, tree};
use crate::zarchive::Reader;

/// The root folder of a WUA holding the revert data.
pub const REVERT_DIR: &str = ".nus";

const BLOCK: u64 = 0x10000;
const HASHES: usize = 0x400;
const DATA: u64 = BLOCK - HASHES as u64;
const GROUP: u64 = 4096;
const CHUNK: u64 = 16 * BLOCK;

/// What a title's revert data starts from: its TMD, ticket and certificates,
/// and where its FST is.
pub struct Meta {
    pub tmd: Vec<u8>,
    pub ticket: Vec<u8>,
    pub cert: Option<Vec<u8>>,
    /// Where content 0 starts for `Source::read_cluster`.
    pub fst_start: u64,
}

/// Where each file of a cluster is: its offset in the cluster's data, its
/// size, and, when rebuilding, its offset in the WUA's data stream.
#[derive(Clone, Copy)]
struct Extent {
    start: u64,
    size: u64,
    stream: u64,
}

/// The files of each cluster, sorted.
fn extents(fst: &Fst, tree: &[Item], streams: &[u64]) -> Vec<Vec<Extent>> {
    let mut by_cluster: Vec<Vec<Extent>> = Vec::new();
    for (item, &stream) in tree.iter().filter(|i| !i.is_dir).zip(streams) {
        let (cluster, start, size) = fst.extent(item.entry);
        let cluster = cluster as usize;
        if by_cluster.len() <= cluster {
            by_cluster.resize_with(cluster + 1, Vec::new);
        }
        by_cluster[cluster].push(Extent {
            start,
            size,
            stream,
        });
    }
    for extents in &mut by_cluster {
        extents.sort_by_key(|e| e.start);
    }
    by_cluster
}

/// The extents among `extents` overlapping `[at, at + len)`.
fn overlapping(extents: &[Extent], at: u64, len: u64) -> impl Iterator<Item = &Extent> {
    let first = extents.partition_point(|e| e.start + e.size <= at);
    extents[first..]
        .iter()
        .take_while(move |e| e.start < at + len)
        .filter(|e| e.size > 0)
}

/// Twenty-byte hashes, in an array of 16, zero-padded.
fn pad16(hashes: &[[u8; 20]]) -> [u8; 0x140] {
    let mut out = [0; 0x140];
    for (slot, hash) in out.as_chunks_mut::<20>().0.iter_mut().zip(hashes) {
        *slot = *hash;
    }
    out
}

/// The hash areas of a group of up to 4096 blocks whose data hashes to `h0s`:
/// each holds the H0 hashes of its 16 blocks, the H1 hashes of its 256, and
/// the group's H2 hashes, as the console checks them.
fn hash_areas(h0s: &[[u8; 20]]) -> Vec<[u8; HASHES]> {
    let h1s: Vec<[u8; 20]> = h0s
        .chunks(16)
        .map(|c| Sha1::digest(pad16(c)).into())
        .collect();
    let h2s: Vec<[u8; 20]> = h1s
        .chunks(16)
        .map(|c| Sha1::digest(pad16(c)).into())
        .collect();
    let h2 = pad16(&h2s);
    (0..h0s.len())
        .map(|b| {
            let mut area = [0; HASHES];
            area[..0x140]
                .copy_from_slice(&pad16(&h0s[b / 16 * 16..(b / 16 * 16 + 16).min(h0s.len())]));
            area[0x140..0x280].copy_from_slice(&pad16(
                &h1s[b / 256 * 16..(b / 256 * 16 + 16).min(h1s.len())],
            ));
            area[0x280..0x3C0].copy_from_slice(&h2);
            area
        })
        .collect()
}

/// Whether `digest` is `content`'s TMD hash, SHA-1 or SHA-256 as it says.
fn check(content: &Content, sha1: Sha1, sha256: Sha256) -> Result<()> {
    let matches = match content.flags & SHA1 != 0 {
        true => sha1.finalize().as_slice() == &content.hash[..20],
        false => sha256.finalize().as_slice() == &content.hash[..],
    };
    match matches {
        true => Ok(()),
        false => Err(corrupt(format!(
            "content {:08x} fails its TMD hash",
            content.id
        ))),
    }
}

fn record(out: &mut Vec<u8>, kind: u8, index: u16, at: u64, bytes: &[u8]) {
    out.push(kind);
    out.extend(index.to_be_bytes());
    out.extend(at.to_be_bytes());
    out.extend((bytes.len() as u32).to_be_bytes());
    out.extend(bytes);
}

/// Record the non-zero bytes of `data`, at data offset `at` of content
/// `index`, that no file covers.
fn scan(extents: &[Extent], index: u16, at: u64, data: &[u8], out: &mut Vec<u8>) {
    let mut covered = Vec::new();
    for e in overlapping(extents, at, data.len() as u64) {
        let from = e.start.saturating_sub(at) as usize;
        let to = ((e.start + e.size - at) as usize).min(data.len());
        covered.push((from, to));
    }
    let mut position = 0;
    let mut gaps: Vec<(usize, usize)> = Vec::new();
    for (from, to) in covered {
        if from > position {
            gaps.push((position, from));
        }
        position = position.max(to);
    }
    gaps.push((position, data.len()));
    for (from, to) in gaps {
        let mut i = from;
        while i < to {
            if data[i] == 0 {
                i += 1;
                continue;
            }
            // A run goes on across fewer zeros than a record's header holds.
            let run = i;
            let mut end = i;
            while i < to && i - end < 15 {
                if data[i] != 0 {
                    end = i + 1;
                }
                i += 1;
            }
            i = end;
            record(out, 0, index, at + run as u64, &data[run..end]);
        }
    }
}

/// Read every content of a title, check it against the TMD, and return its
/// revert data, as file names and bytes.
pub fn collect(
    source: &dyn Source,
    fst: &Fst,
    tree: &[Item],
    meta: &Meta,
    report: &mut dyn FnMut(u64),
) -> Result<Vec<(String, Vec<u8>)>> {
    let tmd = nus::parse_tmd(&meta.tmd)?;
    let key = fst.key();
    let files = tree.iter().filter(|i| !i.is_dir).count();
    let extents = extents(fst, tree, &vec![0; files]);
    let mut out = vec![
        ("title.tmd".to_string(), meta.tmd.clone()),
        ("title.tik".to_string(), meta.ticket.clone()),
    ];
    if let Some(cert) = &meta.cert {
        out.push(("title.cert".to_string(), cert.clone()));
    }
    let mut residue = Vec::new();
    let mut buf = vec![0; CHUNK as usize];
    for content in &tmd.contents {
        let start = match content.index {
            0 => Some(meta.fst_start),
            index => fst.cluster_offset(index),
        }
        .ok_or_else(|| corrupt(format!("the title has no content {}", content.index)))?;
        let hashed = content.flags & HASHED != 0;
        if !content.size.is_multiple_of(if hashed { BLOCK } else { 16 }) {
            return Err(corrupt(format!(
                "content {:08x} is {} bytes",
                content.id, content.size
            )));
        }
        let none = Vec::new();
        let extents = extents.get(content.index as usize).unwrap_or(&none);
        let (mut sha1, mut sha256) = (Sha1::new(), Sha256::new());
        if !hashed {
            let mut iv = [0; 16];
            iv[..2].copy_from_slice(&content.index.to_be_bytes());
            let mut encrypted = Vec::new();
            let mut offset = 0;
            while offset < content.size {
                let len = (content.size - offset).min(CHUNK) as usize;
                let chunk = &mut buf[..len];
                source.read_cluster(content.index, start, offset, chunk)?;
                if content.index == 0 {
                    encrypted.extend_from_slice(chunk);
                }
                cbc_decrypt(key, &mut iv, chunk);
                sha1.update(&*chunk);
                sha256.update(&*chunk);
                if content.index != 0 {
                    scan(extents, content.index, offset, chunk, &mut residue);
                }
                offset += len as u64;
                report(len as u64);
            }
            check(content, sha1, sha256)?;
            if content.index == 0 {
                out.push((format!("{:08x}.app", content.id), encrypted));
            }
            continue;
        }
        let blocks = content.size / BLOCK;
        let mut h3 = Vec::new();
        for group in (0..blocks).step_by(GROUP as usize) {
            let count = (blocks - group).min(GROUP);
            let mut areas = Vec::with_capacity(count as usize);
            let mut h0s = Vec::with_capacity(count as usize);
            for chunk_start in (group..group + count).step_by((CHUNK / BLOCK) as usize) {
                let n = (group + count - chunk_start).min(CHUNK / BLOCK);
                let chunk = &mut buf[..(n * BLOCK) as usize];
                source.read_cluster(content.index, start, chunk_start * BLOCK, chunk)?;
                for (i, block) in chunk.chunks_mut(BLOCK as usize).enumerate() {
                    let b = chunk_start + i as u64;
                    let (area, data) = block.split_at_mut(HASHES);
                    cbc_decrypt(key, &mut [0; 16], area);
                    let h0: [u8; 20] = area[(b % 16) as usize * 20..][..20].try_into().unwrap();
                    cbc_decrypt(key, &mut h0[..16].try_into().unwrap(), data);
                    if Sha1::digest(&*data).as_slice() != h0 {
                        return Err(corrupt(format!(
                            "hashed block {b} of content {:08x} fails its hash check",
                            content.id
                        )));
                    }
                    scan(extents, content.index, b * DATA, data, &mut residue);
                    areas.push(<[u8; HASHES]>::try_from(&*area).unwrap());
                    h0s.push(h0);
                }
                report(n * BLOCK);
            }
            for (i, (rebuilt, area)) in hash_areas(&h0s).iter().zip(&areas).enumerate() {
                if rebuilt != area {
                    record(&mut residue, 1, content.index, group + i as u64, area);
                }
            }
            h3.extend(Sha1::digest(&areas[0][0x280..0x3C0]));
        }
        check(
            content,
            Sha1::new_with_prefix(&h3),
            Sha256::new_with_prefix(&h3),
        )?;
    }
    out.push(("residue".to_string(), residue));
    Ok(out)
}

/// A content's residue: its bytes outside files, by data offset, and its hash
/// areas that are not rebuilt, by block.
#[derive(Default)]
struct Residue {
    bytes: Vec<(u64, Vec<u8>)>,
    areas: Vec<(u64, [u8; HASHES])>,
}

fn parse_residue(bytes: &[u8]) -> Result<Vec<Residue>> {
    let mut out: Vec<Residue> = Vec::new();
    let mut at = 0;
    let short = || corrupt("the revert data's residue is cut short");
    while at < bytes.len() {
        let header = bytes.get(at..at + 15).ok_or_else(short)?;
        let kind = header[0];
        let index = u16::from_be_bytes([header[1], header[2]]) as usize;
        let offset = u64::from_be_bytes(header[3..11].try_into().unwrap());
        let len = u32::from_be_bytes(header[11..15].try_into().unwrap()) as usize;
        let data = bytes.get(at + 15..at + 15 + len).ok_or_else(short)?;
        at += 15 + len;
        if out.len() <= index {
            out.resize_with(index + 1, Residue::default);
        }
        match (kind, <[u8; HASHES]>::try_from(data)) {
            (0, _) => out[index].bytes.push((offset, data.to_vec())),
            (1, Ok(area)) => out[index].areas.push((offset, area)),
            _ => return Err(corrupt("the revert data's residue has an unknown record")),
        }
    }
    for residue in &mut out {
        residue.bytes.sort_by_key(|(offset, _)| *offset);
    }
    Ok(out)
}

/// Fill `buf` with a content's data at data offset `at`: its files' bytes,
/// its residue, and zeros.
fn fill(
    reader: &mut Reader,
    extents: &[Extent],
    residue: &Residue,
    at: u64,
    buf: &mut [u8],
) -> Result<()> {
    buf.fill(0);
    let len = buf.len() as u64;
    for e in overlapping(extents, at, len) {
        let from = e.start.max(at);
        let to = (e.start + e.size).min(at + len);
        reader.read(
            e.stream + (from - e.start),
            &mut buf[(from - at) as usize..(to - at) as usize],
        )?;
    }
    let first = residue
        .bytes
        .partition_point(|(offset, bytes)| offset + bytes.len() as u64 <= at);
    for (offset, bytes) in residue.bytes[first..]
        .iter()
        .take_while(|(offset, _)| *offset < at + len)
    {
        let from = (*offset).max(at);
        let to = (offset + bytes.len() as u64).min(at + len);
        buf[(from - at) as usize..(to - at) as usize]
            .copy_from_slice(&bytes[(from - offset) as usize..(to - offset) as usize]);
    }
    Ok(())
}

/// Rebuild every title of the WUA at `input` as a NUS folder, the form
/// Nintendo's servers deliver titles in and WUP Installer GX2 installs, each
/// in `output/<title id>_v<version>/`. Every content is byte for byte the one
/// the WUA was made from, and checked against its TMD.
///
/// Only WUAs written with revert data can be; `keys` must hold the Wii U
/// common key. `progress` is called with input bytes, scaled from the content
/// bytes rebuilt. The folder is written as `<output>.part` and renamed into
/// place once complete.
pub fn wua_to_nus(
    input: &Path,
    output: &Path,
    keys: &[Key],
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    let mut reader = Reader::open(input)?;
    let revert = reader.lookup(0, REVERT_DIR).ok_or_else(|| {
        Error::Unsupported("a WUA without revert data, which only WUAs written with it hold".into())
    })?;
    let titles = reader.children(revert);
    let mut tmds = Vec::with_capacity(titles.len());
    for (folder, node) in &titles {
        let tmd = reader
            .lookup(*node, "title.tmd")
            .ok_or_else(|| corrupt(format!("the revert data of {folder} has no TMD")))?;
        tmds.push(reader.read_file(tmd)?);
    }
    let total: u64 = tmds
        .iter()
        .map(|tmd| nus::parse_tmd(tmd).map(|t| t.contents.iter().map(|c| c.size).sum::<u64>()))
        .sum::<Result<u64>>()?;
    let input_size = std::fs::metadata(input)?.len();

    let part = part_dir(output);
    if part.exists() {
        std::fs::remove_dir_all(&part)?;
    }
    let result = (|| {
        std::fs::create_dir_all(&part)?;
        let (mut done, mut reported) = (0u64, 0u64);
        let mut report = |n: u64| {
            done += n;
            let scaled =
                (u128::from(done) * u128::from(input_size) / u128::from(total.max(1))) as u64;
            progress(scaled - reported);
            reported = scaled;
        };
        for ((folder, node), tmd) in titles.iter().zip(&tmds) {
            rebuild(
                &mut reader,
                folder,
                *node,
                tmd,
                keys,
                &part.join(folder),
                &mut report,
            )?;
        }
        progress(input_size - reported);
        let mut output_size = 0;
        for title in std::fs::read_dir(&part)? {
            for entry in std::fs::read_dir(title?.path())? {
                output_size += entry?.metadata()?.len();
            }
        }
        Ok(Stats {
            input_size,
            output_size,
        })
    })();
    match result {
        Ok(stats) => {
            if output.exists() {
                std::fs::remove_dir_all(output)?;
            }
            std::fs::rename(&part, output)?;
            Ok(stats)
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&part);
            Err(e)
        }
    }
}

/// Rebuild the title whose revert data is at `node`, and whose files are in
/// the WUA's `folder`, into the folder `output`.
fn rebuild(
    reader: &mut Reader,
    folder: &str,
    node: usize,
    tmd_bytes: &[u8],
    keys: &[Key],
    output: &Path,
    report: &mut dyn FnMut(u64),
) -> Result<()> {
    let file = |reader: &mut Reader, name: &str| -> Result<Option<Vec<u8>>> {
        reader
            .lookup(node, name)
            .map(|file| reader.read_file(file))
            .transpose()
    };
    let missing = |name: &str| corrupt(format!("the revert data of {folder} has no {name}"));
    let tmd = nus::parse_tmd(tmd_bytes)?;
    let ticket = file(reader, "title.tik")?.ok_or_else(|| missing("ticket"))?;
    let cert = file(reader, "title.cert")?;
    let residues = parse_residue(&file(reader, "residue")?.ok_or_else(|| missing("residue"))?)?;
    let fst_content = tmd
        .contents
        .iter()
        .find(|c| c.index == 0)
        .ok_or_else(|| corrupt(format!("the TMD of {folder} lists no FST")))?;
    let fst_name = format!("{:08x}.app", fst_content.id);
    let content0 = file(reader, &fst_name)?.ok_or_else(|| missing("FST"))?;

    let (title_id, encrypted_key) = parse_ticket(&ticket)?;
    let key = title_key(&content0, title_id, &encrypted_key, keys)?;
    let size = u32::try_from(fst_content.size)
        .map_err(|_| corrupt(format!("the FST is {} bytes", fst_content.size)))?;
    let fst = Fst::open(folder, content0.clone(), size, key)?;

    // Each file's place in the WUA, by the path the FST gives it.
    let tree = tree(&fst);
    let title = reader
        .lookup(0, folder)
        .ok_or_else(|| corrupt(format!("the WUA has no {folder}")))?;
    let mut paths = vec![String::new(); fst.entries.len()];
    let mut streams = Vec::new();
    for item in &tree {
        let name = String::from_utf8_lossy(fst.name(item.entry)?).into_owned();
        paths[item.entry] = match item.parent {
            0 => name,
            parent => format!("{}/{name}", paths[parent]),
        };
        if !item.is_dir {
            let (stream, size) = reader
                .lookup(title, &paths[item.entry])
                .and_then(|file| reader.file(file))
                .ok_or_else(|| corrupt(format!("the WUA has no {folder}/{}", paths[item.entry])))?;
            if size != fst.extent(item.entry).2 {
                return Err(corrupt(format!(
                    "{folder}/{} is not the size the FST says",
                    paths[item.entry]
                )));
            }
            streams.push(stream);
        }
    }
    let extents = extents(&fst, &tree, &streams);

    std::fs::create_dir_all(output)?;
    std::fs::write(output.join("title.tmd"), tmd_bytes)?;
    std::fs::write(output.join("title.tik"), &ticket)?;
    if let Some(cert) = &cert {
        std::fs::write(output.join("title.cert"), cert)?;
    }
    let mut buf = vec![0; CHUNK as usize];
    let none_extents = Vec::new();
    let none_residue = Residue::default();
    for content in &tmd.contents {
        let path = output.join(format!("{:08x}.app", content.id));
        if content.index == 0 {
            std::fs::write(&path, &content0)?;
            report(content.size);
            continue;
        }
        let extents = extents.get(content.index as usize).unwrap_or(&none_extents);
        let residue = residues
            .get(content.index as usize)
            .unwrap_or(&none_residue);
        let mut out = BufWriter::new(File::create(&path)?);
        let (mut sha1, mut sha256) = (Sha1::new(), Sha256::new());
        if content.flags & HASHED == 0 {
            let mut iv = [0; 16];
            iv[..2].copy_from_slice(&content.index.to_be_bytes());
            let mut offset = 0;
            while offset < content.size {
                let len = (content.size - offset).min(CHUNK) as usize;
                let chunk = &mut buf[..len];
                fill(reader, extents, residue, offset, chunk)?;
                sha1.update(&*chunk);
                sha256.update(&*chunk);
                encrypt(&key, &mut iv, chunk);
                out.write_all(chunk)?;
                offset += len as u64;
                report(len as u64);
            }
            out.flush()?;
            check(content, sha1, sha256)?;
            continue;
        }
        let blocks = content.size / BLOCK;
        let mut h3 = Vec::new();
        let mut data = vec![0; DATA as usize];
        for group in (0..blocks).step_by(GROUP as usize) {
            let count = (blocks - group).min(GROUP);
            // The hash areas need the whole group's data first.
            let mut h0s = Vec::with_capacity(count as usize);
            for b in group..group + count {
                fill(reader, extents, residue, b * DATA, &mut data)?;
                h0s.push(Sha1::digest(&data).into());
            }
            let mut areas = hash_areas(&h0s);
            for (b, area) in &residue.areas {
                if (group..group + count).contains(b) {
                    areas[(b - group) as usize] = *area;
                }
            }
            h3.extend(Sha1::digest(&areas[0][0x280..0x3C0]));
            for (i, area) in areas.iter().enumerate() {
                let b = group + i as u64;
                fill(reader, extents, residue, b * DATA, &mut data)?;
                let iv: [u8; 16] = area[(b % 16) as usize * 20..][..16].try_into().unwrap();
                let mut encrypted_area = *area;
                encrypt(&key, &mut [0; 16], &mut encrypted_area);
                encrypt(&key, &mut iv.clone(), &mut data);
                out.write_all(&encrypted_area)?;
                out.write_all(&data)?;
                report(BLOCK);
            }
        }
        out.flush()?;
        std::fs::write(output.join(format!("{:08x}.h3", content.id)), &h3)?;
        check(
            content,
            Sha1::new_with_prefix(&h3),
            Sha256::new_with_prefix(&h3),
        )?;
    }
    Ok(())
}

/// Encrypt `buf` in place with AES-128-CBC, continuing the chain from `iv`.
fn encrypt(key: &Key, iv: &mut [u8; 16], buf: &mut [u8]) {
    use aes::Aes128;
    use aes::cipher::{BlockCipherEncrypt, KeyInit};
    let cipher = Aes128::new(key.into());
    for block in buf.as_chunks_mut::<16>().0 {
        for (b, c) in block.iter_mut().zip(iv.iter()) {
            *b ^= c;
        }
        cipher.encrypt_block(block.into());
        *iv = *block;
    }
}

/// Where the folder `output` is written until it is complete.
fn part_dir(output: &Path) -> PathBuf {
    let mut name = output.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    output.with_file_name(name)
}

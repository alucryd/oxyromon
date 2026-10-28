//! WUD or WUX to a NUS folder, the form Nintendo's servers deliver titles in
//! and WUP Installer GX2 installs: the disc's own TMD, ticket and
//! certificates, and each content copied as it is encrypted on the disc, with
//! the H3 hashes of hashed ones.
//!
//! A disc's game partition holds the title's CDN contents byte for byte, but
//! for content 0, the FST, which the disc lays out for itself; its TMD,
//! signed by Nintendo for the disc, says so. The folder installs, but its FST
//! and TMD differ from the CDN's.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use sha1::{Digest, Sha1};
use sha2::Sha256;

use crate::Stats;
use crate::crypto::Key;
use crate::crypto::cbc_decrypt;
use crate::disc::Source;
use crate::error::{Result, corrupt};
use crate::io::{finish, finish_dir, part};
use crate::nus::{self, Content, SHA1};
use crate::wua::{DiscGame, open_nus_title};

/// A hashed block, and how many share the H2 hashes that make one H3 hash.
const BLOCK: u64 = 0x10000;
const BLOCKS_PER_H3: u64 = 4096;

/// Write the game on the WUD or WUX at `input` to the folder `output`, as
/// `title.tmd`, `title.tik`, `title.cert`, `<content id>.app` and, for hashed
/// contents, `<content id>.h3`. Each content is checked against its TMD hash.
///
/// `keys` are candidates, as in Cemu's `keys.txt`: among them must be the
/// disc's key and the Wii U common key. `progress` is called with input
/// bytes, scaled from the content bytes copied. The folder is written as
/// `<output>.part` and renamed into place once complete.
pub fn to_nus(
    input: &Path,
    output: &Path,
    keys: &[crate::Key],
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    let game = DiscGame::open(input, keys)?;
    let tmd_bytes = game.si_file("title.tmd")?;
    let tmd = nus::parse_tmd(&tmd_bytes)?;
    let input_size = std::fs::metadata(input)?.len();
    let total: u64 = tmd.contents.iter().map(|c| c.size).sum();

    let part = part(output);
    if part.exists() {
        std::fs::remove_dir_all(&part)?;
    }
    let result = (|| {
        std::fs::create_dir_all(&part)?;
        std::fs::write(part.join("title.tmd"), &tmd_bytes)?;
        std::fs::write(part.join("title.tik"), game.si_file("title.tik")?)?;
        std::fs::write(part.join("title.cert"), game.si_file("title.cert")?)?;
        let (mut done, mut reported) = (0u64, 0u64);
        let mut buf = vec![0; 16 * BLOCK as usize];
        for content in &tmd.contents {
            // Content 0 is the FST, which says cluster 0 starts at the
            // partition's start; it is where the partition's header says.
            let start = match content.index {
                0 => Some(game.fst_start),
                index => game.fst.cluster_offset(index),
            }
            .ok_or_else(|| {
                corrupt(format!(
                    "the game partition has no content {}",
                    content.index
                ))
            })?;
            let hashed = content.is_hashed();
            if !content.size.is_multiple_of(if hashed { BLOCK } else { 16 }) {
                return Err(corrupt(format!(
                    "content {:08x} is {} bytes",
                    content.id, content.size
                )));
            }
            let mut file =
                BufWriter::new(File::create(part.join(format!("{:08x}.app", content.id)))?);
            let mut iv = [0; 16];
            iv[..2].copy_from_slice(&content.index.to_be_bytes());
            let (mut sha1, mut sha256) = (Sha1::new(), Sha256::new());
            let mut h3 = Vec::new();
            let mut offset = 0;
            while offset < content.size {
                let len = (content.size - offset).min(buf.len() as u64) as usize;
                let chunk = &mut buf[..len];
                game.source
                    .read_cluster(content.index, start, offset, chunk)?;
                file.write_all(chunk)?;
                if hashed {
                    // The first block of each group holds the group's H2 hashes.
                    for (i, block) in chunk.chunks(BLOCK as usize).enumerate() {
                        if (offset / BLOCK + i as u64).is_multiple_of(BLOCKS_PER_H3) {
                            h3.extend(h3_hash(&game.title_key, block));
                        }
                    }
                } else {
                    cbc_decrypt(&game.title_key, &mut iv, chunk);
                    sha1.update(&*chunk);
                    sha256.update(&*chunk);
                }
                offset += len as u64;
                done += len as u64;
                let scaled =
                    (u128::from(done) * u128::from(input_size) / u128::from(total.max(1))) as u64;
                progress(scaled - reported);
                reported = scaled;
            }
            file.flush()?;
            if hashed {
                std::fs::write(part.join(format!("{:08x}.h3", content.id)), &h3)?;
                sha1 = Sha1::new_with_prefix(&h3);
                sha256 = Sha256::new_with_prefix(&h3);
            }
            let matches = match content.flags & SHA1 != 0 {
                true => sha1.finalize().as_slice() == &content.hash[..20],
                false => sha256.finalize().as_slice() == &content.hash[..],
            };
            if !matches {
                return Err(corrupt(format!(
                    "content {:08x} fails its TMD hash",
                    content.id
                )));
            }
        }
        progress(input_size - reported);
        let mut output_size = 0;
        for entry in std::fs::read_dir(&part)? {
            output_size += entry?.metadata()?.len();
        }
        Ok(Stats {
            input_size,
            output_size,
        })
    })();
    finish_dir(&part, output, result)
}

/// Write, in the NUS folder at `dir`, the `<content id>.h3` of each hashed
/// content its TMD lists but it lacks, each checked against the TMD, and
/// return the files written. An H3 hash is the SHA-1 of a group of blocks'
/// H2 hashes, which the contents hold encrypted with the title key: a title
/// installs only with every H3 file, which No-Intro sometimes leaves out.
///
/// `keys` are needed only when an H3 file is missing, and are candidates as
/// in Cemu's `keys.txt`: among them must be the Wii U common key. A folder
/// whose TMD is not a Wii U title's, as a vWii title's, is left alone.
pub fn write_missing_h3(dir: &Path, keys: &[Key]) -> Result<Vec<PathBuf>> {
    let Some(tmd_path) = nus::tmd_path(dir) else {
        return Ok(Vec::new());
    };
    let tmd_bytes = std::fs::read(tmd_path)?;
    // The title ID is where it is in a Wii TMD too, whose records differ.
    let is_wiiu = tmd_bytes
        .get(0x18C..0x194)
        .is_some_and(|id| u64::from_be_bytes(id.try_into().unwrap()) >> 48 == 5);
    if !is_wiiu {
        return Ok(Vec::new());
    }
    let tmd = nus::parse_tmd(&tmd_bytes)?;
    // Downloaders write the IDs in upper case, No-Intro in lower case.
    let present: HashSet<String> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .map(|name| name.to_lowercase())
        .collect();
    let missing: Vec<&Content> = tmd
        .contents
        .iter()
        .filter(|c| c.is_hashed() && !present.contains(&format!("{:08x}.h3", c.id)))
        .collect();
    if missing.is_empty() {
        return Ok(Vec::new());
    }
    let title = open_nus_title(dir, keys, &[])?;
    let mut written = Vec::with_capacity(missing.len());
    let mut head = vec![0; 0x400];
    for content in missing {
        if !content.size.is_multiple_of(BLOCK) {
            return Err(corrupt(format!(
                "content {:08x} is {} bytes",
                content.id, content.size
            )));
        }
        // The first block of each group holds the group's H2 hashes.
        let mut h3 = Vec::new();
        for offset in (0..content.size).step_by((BLOCK * BLOCKS_PER_H3) as usize) {
            title
                .source
                .read_cluster(content.index, 0, offset, &mut head)?;
            h3.extend(h3_hash(&title.title_key, &head));
        }
        let matches = match content.flags & SHA1 != 0 {
            true => Sha1::digest(&h3).as_slice() == &content.hash[..20],
            false => Sha256::digest(&h3).as_slice() == &content.hash[..],
        };
        if !matches {
            return Err(corrupt(format!(
                "content {:08x} fails its TMD hash",
                content.id
            )));
        }
        let path = dir.join(format!("{:08x}.h3", content.id));
        let part = part(&path);
        finish(&part, &path, std::fs::write(&part, &h3).map_err(Into::into))?;
        written.push(path);
    }
    Ok(written)
}

/// The H3 hash of a group of hashed blocks, from its first block: the SHA-1
/// of the group's H2 hashes, in the block's encrypted hash area.
fn h3_hash(title_key: &Key, block: &[u8]) -> [u8; 20] {
    let mut hashes = block[..0x400].to_vec();
    cbc_decrypt(title_key, &mut [0; 16], &mut hashes);
    Sha1::digest(&hashes[0x280..0x3C0]).into()
}

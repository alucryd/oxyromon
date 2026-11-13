//! WBFS creation and extraction for Wii and GameCube discs, part of oxyROMon.
//!
//! WBFS is the disc image format of the Wii's USB loaders: a disc cut into 2
//! MiB blocks, only those holding something stored, behind a table mapping
//! each block of the disc to where it is stored. This is a port of how [wit]
//! writes one, `wit copy --wbfs`: the 32 KiB clusters nothing on the disc
//! points to are left out, or zeroed where they share a block with something,
//! so a WBFS is scrubbed, and no longer the disc it was. It is wit's byte for
//! byte but for a leftover table no reader looks at, and for discs without a
//! boot DOL, which wit breaks and wbfs-rs keeps whole.
//!
//! ```no_run
//! use std::path::Path;
//!
//! // Finding what a Wii disc uses needs the Wii common key, which ships with
//! // nothing here
//! let keys = wbfs_rs::load_keys(Path::new("keys.bin")).unwrap();
//! wbfs_rs::create(Path::new("game.iso"), Path::new("game.wbfs"), &keys, &mut |_| {}).unwrap();
//! wbfs_rs::extract(Path::new("game.wbfs"), Path::new("out.iso"), &mut |_| {}).unwrap();
//! ```
//!
//! [wit]: https://wit.wiimm.de

// The GameCube and Wii disc specifics, shared with rvz-rs: `src/gcwii` is a
// symlink to the gcwii crate's source, which publishing copies in
#[allow(dead_code, unused_imports)]
#[path = "gcwii/lib.rs"]
mod gcwii;

use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use gcwii::disc::Iso;
use gcwii::io::{finish, part, read_exact_at};
use gcwii::scrub::used_clusters;

pub use gcwii::crypto::Key;
pub use gcwii::disc::Source;
pub use gcwii::error::{Error, Result};
pub use gcwii::keys::load_keys;

const MAGIC: [u8; 4] = *b"WBFS";
/// wit's sectors, its unit of the file's size: 512 bytes.
const SECTOR_SHIFT: u32 = 9;
/// wit's blocks for a WBFS file: 2 MiB, the smallest that addresses a
/// dual-layer disc with 16 bits.
const BLOCK_SHIFT: u32 = 21;
pub const BLOCK_SIZE: u64 = 1 << BLOCK_SHIFT;
const SECTORS_PER_BLOCK: u64 = BLOCK_SIZE >> SECTOR_SHIFT;
const CLUSTER_SIZE: u64 = gcwii::scrub::CLUSTER_SIZE;
const CLUSTER_SHIFT: u32 = CLUSTER_SIZE.trailing_zeros();
/// A single-layer Wii disc, in clusters, and the largest disc, a dual-layer
/// one, as wit counts them.
const SINGLE_LAYER_CLUSTERS: u64 = 143_432;
const MAX_CLUSTERS: u64 = 2 * SINGLE_LAYER_CLUSTERS;
const MAX_DISC_SIZE: u64 = MAX_CLUSTERS * CLUSTER_SIZE;
const GAMECUBE_DISC_SIZE: u64 = 1_459_978_240;
const GAMECUBE_MAGIC: u32 = 0xC2339F3D;
/// The disc header a WBFS keeps a copy of, after its own header.
const DISC_HEADER_SIZE: usize = 0x100;
/// Sizes of the two files a run touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub input_size: u64,
    pub output_size: u64,
}

/// Whether `path` is a WBFS, by its magic.
pub fn is_wbfs(path: &Path) -> Result<bool> {
    let file = File::open(path)?;
    let mut magic = [0; 4];
    match read_exact_at(&file, &mut magic, 0) {
        Ok(()) => Ok(magic == MAGIC),
        // Too short to hold a magic: not a WBFS, but an unreadable file is
        // not one either way
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// Write `input`, a raw GameCube or Wii disc, to `output` as a WBFS. `keys`
/// are the Wii common keys to decrypt a Wii disc's partitions with, to find
/// what they use; a GameCube disc needs none.
///
/// `progress` is called with the input bytes consumed since the last call.
pub fn create(
    input: &Path,
    output: &Path,
    keys: &[Key],
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    create_from(&mut Iso::open(input)?, output, keys, progress)
}

/// Write the disc `source` reads to `output` as a WBFS, as [`create`] does.
/// A source keeping its Wii partitions' title keys, as an RVZ does, needs no
/// common key.
///
/// `progress` is called with the disc's bytes consumed since the last call.
pub fn create_from(
    source: &mut dyn Source,
    output: &Path,
    keys: &[Key],
    progress: &mut dyn FnMut(u64),
) -> Result<Stats> {
    let disc_size = source.size();
    if disc_size > MAX_DISC_SIZE {
        return Err(Error::Unsupported(format!(
            "a disc of {disc_size} bytes, larger than a dual-layer Wii disc"
        )));
    }
    let used = used_clusters(source, keys)?;

    // Each block of the disc holding a used cluster is stored, in order
    let clusters_per_block = (BLOCK_SIZE / CLUSTER_SIZE) as usize;
    let mut table = vec![0u16; (MAX_CLUSTERS / clusters_per_block as u64) as usize];
    let mut stored = 0u16;
    for (block, clusters) in used.chunks(clusters_per_block).enumerate() {
        if clusters.contains(&true) {
            stored += 1;
            table[block] = stored;
        }
    }
    let blocks = stored as u64 + 1;

    let part = part(output);
    let result = (|| {
        let mut file = File::create(&part)?;
        let mut disc_header = [0; DISC_HEADER_SIZE];
        source.read_at(0, &mut disc_header)?;
        file.write_all(&header(blocks, &disc_header, &table))?;
        let mut buf = vec![0; BLOCK_SIZE as usize];
        for (block, &stored) in table.iter().enumerate() {
            let offset = block as u64 * BLOCK_SIZE;
            if offset >= disc_size {
                break;
            }
            let length = BLOCK_SIZE.min(disc_size - offset);
            if stored != 0 {
                buf.fill(0);
                source.read_at(offset, &mut buf[..length as usize])?;
                // Zero the clusters nothing points to
                let first = block * clusters_per_block;
                for (i, cluster) in buf.chunks_mut(CLUSTER_SIZE as usize).enumerate() {
                    if !used.get(first + i).copied().unwrap_or(false) {
                        cluster.fill(0);
                    }
                }
                file.write_all(&buf)?;
            }
            progress(length);
        }
        file.sync_all()?;
        Ok(Stats {
            input_size: disc_size,
            output_size: file.metadata()?.len(),
        })
    })();
    finish(&part, output, result)
}

/// The first block of a WBFS of `blocks` blocks, the first included.
fn header(blocks: u64, disc_header: &[u8; DISC_HEADER_SIZE], table: &[u16]) -> Vec<u8> {
    let mut header = vec![0; BLOCK_SIZE as usize];
    header[0..4].copy_from_slice(&MAGIC);
    header[4..8].copy_from_slice(&((blocks * SECTORS_PER_BLOCK) as u32).to_be_bytes());
    header[8] = SECTOR_SHIFT as u8;
    header[9] = BLOCK_SHIFT as u8;
    header[10] = 1; // version
    header[12] = 1; // the first slot holds a disc
    let disc = 1 << SECTOR_SHIFT;
    header[disc..disc + DISC_HEADER_SIZE].copy_from_slice(disc_header);
    for (i, &stored) in table.iter().enumerate() {
        let at = disc + DISC_HEADER_SIZE + 2 * i;
        header[at..at + 2].copy_from_slice(&stored.to_be_bytes());
    }

    // The table of free blocks, at the end of this block, is left zeroed:
    // every block stored is used, and a clear bit is a used block
    header
}

/// Write a WBFS out as the disc it holds, its scrubbed clusters as zeros: a
/// Wii disc as long as a single-layer one at least, as wit writes it, and a
/// GameCube disc as long as one.
///
/// `progress` is called with the input bytes consumed since the last call.
pub fn extract(input: &Path, output: &Path, progress: &mut dyn FnMut(u64)) -> Result<Stats> {
    let source = File::open(input)?;
    let input_size = source.metadata()?.len();
    let corrupt = |what: &str| Error::Corrupt(format!("{}: {what}", input.display()));

    let mut head = [0; 16];
    read_exact_at(&source, &mut head, 0)?;
    if head[0..4] != MAGIC {
        return Err(Error::BadMagic {
            expected: "WBFS".into(),
            found: head[0..4].escape_ascii().to_string(),
        });
    }
    let sectors = u32::from_be_bytes(head[4..8].try_into().unwrap()) as u64;
    let (sector_shift, block_shift) = (head[8] as u32, head[9] as u32);
    if !(SECTOR_SHIFT..=12).contains(&sector_shift)
        || !(CLUSTER_SHIFT..=30).contains(&block_shift)
        || block_shift < sector_shift
    {
        return Err(corrupt("bad sector or block size"));
    }
    if head[10] != 1 {
        return Err(Error::Unsupported(format!("WBFS version {}", head[10])));
    }
    if sectors << sector_shift != input_size {
        return Err(corrupt("file size mismatch"));
    }
    // The table of discs, a byte a slot to the end of the first sector: a
    // WBFS file holds one disc, in the first
    let mut slots = vec![0; (1 << sector_shift) - 12];
    read_exact_at(&source, &mut slots, 12)?;
    if slots[1..].iter().any(|&slot| slot != 0) {
        return Err(Error::Unsupported(
            "a WBFS partition holding several discs".into(),
        ));
    }
    match slots[0] {
        1 => {}
        0 => return Err(Error::Unsupported("a WBFS holding no disc".into())),
        slot => return Err(Error::Unsupported(format!("a WBFS disc slot of {slot}"))),
    }
    let block_size = 1u64 << block_shift;
    let blocks_in_file = input_size >> block_shift;

    // The disc header, then the table of where each block is stored
    let entries = (MAX_CLUSTERS >> (block_shift - CLUSTER_SHIFT)) as usize;
    let mut disc = vec![0; DISC_HEADER_SIZE + 2 * entries];
    read_exact_at(&source, &mut disc, 1 << sector_shift)?;
    let table: Vec<u64> = disc[DISC_HEADER_SIZE..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|entry| u16::from_be_bytes(*entry) as u64)
        .collect();
    if table.iter().any(|&stored| stored >= blocks_in_file) {
        return Err(corrupt("block stored past the end of the file"));
    }
    let gamecube = u32::from_be_bytes(disc[0x1C..0x20].try_into().unwrap()) == GAMECUBE_MAGIC;
    // wit pads a GameCube disc as it pads a Wii one, to a single-layer Wii
    // disc; a GameCube disc is always the same size, so it gets that instead
    let last = table.iter().rposition(|&stored| stored != 0).unwrap_or(0);
    let output_size = if gamecube {
        GAMECUBE_DISC_SIZE
    } else {
        ((last as u64 + 1) * block_size).max(SINGLE_LAYER_CLUSTERS * CLUSTER_SIZE)
    };

    let part = part(output);
    let result = (|| {
        let mut file = File::create(&part)?;
        progress(block_size);
        // Sized at the first stored block, which the table check bounds by
        // the file's size: a header alone cannot make it gigabytes long
        let mut buf = Vec::new();
        for (block, &stored) in table.iter().enumerate() {
            let offset = block as u64 * block_size;
            if offset >= output_size {
                break;
            }
            // What is not stored is left a hole, which reads as zeros
            if stored != 0 {
                let length = block_size.min(output_size - offset) as usize;
                buf.resize(block_size as usize, 0);
                read_exact_at(&source, &mut buf, stored * block_size)?;
                file.seek(SeekFrom::Start(offset))?;
                file.write_all(&buf[..length])?;
                progress(block_size);
            }
        }
        file.set_len(output_size)?;
        file.sync_all()?;
        Ok(Stats {
            input_size,
            output_size,
        })
    })();
    finish(&part, output, result)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first block as wit wrote it for two real discs, but for what wit
    /// leaves of a table of free blocks it no longer reads.
    fn assert_header_as_wit_wrote(blocks: u64, size: [u8; 4]) {
        let mut table = vec![0; 4482];
        for (block, stored) in table.iter_mut().zip(1..blocks as u16) {
            *block = stored;
        }
        let header = header(blocks, &[0x42; DISC_HEADER_SIZE], &table);
        assert_eq!(header[0..4], *b"WBFS");
        assert_eq!(header[4..8], size);
        assert_eq!(header[8..16], [9, 21, 1, 0, 1, 0, 0, 0]);
        assert!(header[16..0x200].iter().all(|&byte| byte == 0));
        assert!(header[0x200..0x300].iter().all(|&byte| byte == 0x42));
        assert_eq!(header[0x300..0x304], [0, 1, 0, 2]);
        assert!(header[0x2604..].iter().all(|&byte| byte == 0));
    }

    #[test]
    fn a_single_layer_wii_discs_header_is_wits() {
        // Pandora no Tou: 1439 blocks stored, a 2880 MiB WBFS
        assert_header_as_wit_wrote(1440, [0x00, 0x5A, 0x00, 0x00]);
    }

    #[test]
    fn a_gamecube_discs_header_is_wits() {
        // Ikaruga: 104 blocks stored, a 210 MiB WBFS
        assert_header_as_wit_wrote(105, [0x00, 0x06, 0x90, 0x00]);
    }
}

// SPDX-License-Identifier: MIT OR Apache-2.0
//! A sealed segment's parity: Reed-Solomon RS(255, 239) over GF(2^8), as
//! design/01 and design/03 specify, or RS(247, 239).
//!
//! The segment's bytes are cut, in order, into 239 data shards of `shard`
//! bytes each, zero past the segment's end, and the parity shards follow
//! the segment. A codeword is a row: the 4 KiB block at the same place in
//! every shard. Each block has a CRC-32C, so a damaged block is an erasure
//! in its row, and a row is repaired whatever the damage, as long as it
//! loses no more blocks than there are parity shards: a burst of up to
//! that many shards long, or bad sectors anywhere, a few a row.
//!
//! After the segment come its parity shards, its table of CRCs twice, and
//! a trailer: `ORSP`, version, parity shard count, the segment's length and
//! the shard size, and the trailer's CRC-32C.

use std::fs::File;
use std::io::{Seek, SeekFrom, Write};

use rayon::prelude::*;
use reed_solomon_erasure::galois_8::ReedSolomon;

use super::error::{Error, Result};
use super::io;

pub const BLOCK: u64 = 4096;
pub const DATA_SHARDS: u64 = 239;
const MAGIC: [u8; 4] = *b"ORSP";
const VERSION: u16 = 1;
const TRAILER: u64 = 32;
/// How much of a shard a core encodes at a time.
const COLUMN: usize = 256 << 10;

/// Where a sealed segment's parts lie.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    /// The length of what the parity covers: the segment, header and
    /// groups.
    pub data_len: u64,
    pub parity: u64,
    pub shard: u64,
}

impl Layout {
    pub fn new(data_len: u64, parity: u8) -> Self {
        Self {
            data_len,
            parity: parity as u64,
            shard: data_len
                .div_ceil(DATA_SHARDS)
                .next_multiple_of(BLOCK)
                .max(BLOCK),
        }
    }

    fn rows(&self) -> u64 {
        self.shard / BLOCK
    }

    fn data_blocks(&self) -> u64 {
        self.data_len.div_ceil(BLOCK)
    }

    fn blocks(&self) -> u64 {
        self.data_blocks() + self.parity * self.rows()
    }

    fn table_len(&self) -> u64 {
        4 * self.blocks()
    }

    /// What sealing adds to the segment.
    pub fn footer_len(&self) -> u64 {
        if self.parity == 0 {
            0
        } else {
            self.parity * self.shard + 2 * self.table_len() + TRAILER
        }
    }

    /// Where block `b` lies, and its length: the segment's last may be
    /// short, and is zero past it.
    fn block(&self, b: u64) -> (u64, usize) {
        let data_blocks = self.data_blocks();
        if b < data_blocks {
            let at = b * BLOCK;
            (at, BLOCK.min(self.data_len - at) as usize)
        } else {
            (self.data_len + (b - data_blocks) * BLOCK, BLOCK as usize)
        }
    }

    /// The row block `b` is in, and its shard.
    fn place(&self, b: u64) -> (u64, u64) {
        let data_blocks = self.data_blocks();
        if b < data_blocks {
            let at = b * BLOCK;
            ((at % self.shard) / BLOCK, at / self.shard)
        } else {
            let q = b - data_blocks;
            (q % self.rows(), DATA_SHARDS + q / self.rows())
        }
    }

    /// The block of `shard` in `row`, if the segment holds one there.
    fn at(&self, row: u64, shard: u64) -> Option<u64> {
        if shard < DATA_SHARDS {
            let at = shard * self.shard + row * BLOCK;
            (at < self.data_len).then_some(at / BLOCK)
        } else {
            Some(self.data_blocks() + (shard - DATA_SHARDS) * self.rows() + row)
        }
    }

    /// Where the CRC tables start: past the data and its parity.
    pub fn table_at(&self) -> u64 {
        self.data_len + self.parity * self.shard
    }

    fn trailer(&self) -> [u8; TRAILER as usize] {
        let mut out = [0; TRAILER as usize];
        out[0..4].copy_from_slice(&MAGIC);
        out[4..6].copy_from_slice(&VERSION.to_be_bytes());
        out[6] = self.parity as u8;
        out[8..16].copy_from_slice(&self.data_len.to_be_bytes());
        out[16..24].copy_from_slice(&self.shard.to_be_bytes());
        let crc = crc32c::crc32c(&out[..28]);
        out[28..32].copy_from_slice(&crc.to_be_bytes());
        out
    }
}

fn codec(parity: u64) -> Result<ReedSolomon> {
    ReedSolomon::new(DATA_SHARDS as usize, parity as usize)
        .map_err(|e| Error::InvalidOption(format!("parity: {e:?}")))
}

/// Seal the segment `file`, of `data_len` bytes, with `parity` parity
/// shards: append its parity, its CRCs and its trailer.
pub fn seal(file: &File, data_len: u64, parity: u8, progress: &mut dyn FnMut(u64)) -> Result<()> {
    let layout = Layout::new(data_len, parity);
    if layout.parity == 0 {
        return Ok(());
    }
    let rs = codec(layout.parity)?;
    let shard = layout.shard as usize;
    let mut parity = vec![vec![0; shard]; layout.parity as usize];
    let mut crcs = Vec::with_capacity(layout.blocks() as usize);
    let mut buf = vec![0; shard];
    // Shards past the segment's end are zeros, which add nothing
    for i in 0..DATA_SHARDS {
        let start = i * layout.shard;
        if start >= data_len {
            break;
        }
        let len = layout.shard.min(data_len - start) as usize;
        io::read_exact_at(file, &mut buf[..len], start)?;
        progress(len as u64);
        buf[len..].fill(0);
        crcs.extend(buf[..len].chunks(BLOCK as usize).map(crc32c::crc32c));
        encode(&rs, i as usize, &buf, &mut parity)?;
    }
    for shard in &parity {
        crcs.extend(shard.chunks(BLOCK as usize).map(crc32c::crc32c));
    }
    let table: Vec<u8> = crcs.iter().flat_map(|crc| crc.to_be_bytes()).collect();
    let mut out = file;
    out.seek(SeekFrom::Start(data_len))?;
    for shard in &parity {
        out.write_all(shard)?;
    }
    out.write_all(&table)?;
    out.write_all(&table)?;
    out.write_all(&layout.trailer())?;
    Ok(())
}

/// Add data shard `i` to the parity, a column of every shard a core.
fn encode(rs: &ReedSolomon, i: usize, data: &[u8], parity: &mut [Vec<u8>]) -> Result<()> {
    let columns = data.len().div_ceil(COLUMN);
    let mut pieces: Vec<Vec<&mut [u8]>> = (0..columns).map(|_| Vec::new()).collect();
    for shard in parity.iter_mut() {
        for (column, piece) in shard.chunks_mut(COLUMN).enumerate() {
            pieces[column].push(piece);
        }
    }
    pieces
        .into_par_iter()
        .enumerate()
        .try_for_each(|(column, mut pieces)| {
            let at = column * COLUMN;
            rs.encode_single_sep(i, &data[at..at + pieces[0].len()], &mut pieces)
        })
        .map_err(|e| Error::Corrupt(format!("parity: {e:?}")))
}

/// What checking a segment found.
#[derive(Debug, Default)]
pub struct Checked {
    /// Blocks that failed their CRC.
    pub damaged: u64,
    /// Of those, the blocks rebuilt from parity.
    pub repaired: u64,
    /// The stretches of the segment, offset and length, that parity could
    /// not rebuild, or would have but was not asked to.
    pub lost: Vec<(u64, u64)>,
}

/// Check a sealed segment against its CRCs, and with `repair`, rebuild the
/// blocks that fail them from parity, in place, and its CRC tables and
/// trailer if they are damaged. A segment cut short is as damaged as what
/// it lacks.
pub fn check(
    file: &File,
    layout: &Layout,
    repair: bool,
    progress: &mut dyn FnMut(u64),
) -> Result<Checked> {
    let blocks = layout.blocks();
    let table = |copy: u64| -> Vec<u32> {
        let mut bytes = vec![0; layout.table_len() as usize];
        let at = layout.table_at() + copy * layout.table_len();
        // A table cut short or unreadable matches nothing
        let _ = io::read_exact_at(file, &mut bytes, at);
        bytes
            .chunks(4)
            .map(|crc| u32::from_be_bytes(crc.try_into().unwrap()))
            .collect()
    };
    let tables = [table(0), table(1)];

    // Every block's CRC, read a few MiB at a time
    let span = (2048 * BLOCK) as usize;
    let mut crcs = Vec::with_capacity(blocks as usize);
    let mut buf = vec![0; span];
    let mut b = 0;
    while b < blocks {
        let (at, _) = layout.block(b);
        // Reads stop where the segment does, so its last block, perhaps
        // short, stays a block of its own
        let end = if at < layout.data_len {
            layout.data_len
        } else {
            layout.table_at()
        };
        let len = (end - at).min(span as u64) as usize;
        let read = read_some(file, &mut buf[..len], at);
        progress(len as u64);
        for (i, block) in buf[..len].chunks(BLOCK as usize).enumerate() {
            let whole = i * BLOCK as usize + block.len() <= read;
            crcs.push(whole.then(|| crc32c::crc32c(block)));
            b += 1;
        }
    }
    let good = |b: u64, crc: Option<u32>| {
        crc.is_some_and(|crc| tables[0][b as usize] == crc || tables[1][b as usize] == crc)
    };
    let mut damaged: Vec<u64> = (0..blocks)
        .filter(|&b| !good(b, crcs[b as usize]))
        .collect();
    let mut checked = Checked {
        damaged: damaged.len() as u64,
        ..Checked::default()
    };

    // Damage by row
    damaged.sort_by_key(|&b| layout.place(b));
    let rs = codec(layout.parity)?;
    for row in damaged.chunk_by(|&a, &b| layout.place(a).0 == layout.place(b).0) {
        if !repair || row.len() as u64 > layout.parity {
            for &b in row {
                if b < layout.data_blocks() {
                    let (at, len) = layout.block(b);
                    checked.lost.push((at, len as u64));
                }
            }
            continue;
        }
        let number = layout.place(row[0]).0;
        let mut shards: Vec<(Vec<u8>, bool)> = (0..DATA_SHARDS + layout.parity)
            .map(|shard| match layout.at(number, shard) {
                None => (vec![0; BLOCK as usize], true),
                Some(b) if row.contains(&b) => (vec![0; BLOCK as usize], false),
                Some(b) => {
                    let (at, len) = layout.block(b);
                    let mut block = vec![0; BLOCK as usize];
                    let ok = io::read_exact_at(file, &mut block[..len], at).is_ok();
                    (block, ok)
                }
            })
            .collect();
        rs.reconstruct(&mut shards)
            .map_err(|e| Error::Corrupt(format!("parity: {e:?}")))?;
        let mut out = file;
        for &b in row {
            let (_, shard) = layout.place(b);
            let (at, len) = layout.block(b);
            let block = &shards[shard as usize].0[..len];
            out.seek(SeekFrom::Start(at))?;
            out.write_all(block)?;
            crcs[b as usize] = Some(crc32c::crc32c(block));
            checked.repaired += 1;
        }
    }

    // The tables and the trailer, rewritten once the blocks are whole
    if repair && checked.lost.is_empty() {
        let crcs: Vec<u32> = crcs.into_iter().map(|crc| crc.unwrap()).collect();
        let mut trailer = [0; TRAILER as usize];
        let at = layout.table_at() + 2 * layout.table_len();
        let _ = io::read_exact_at(file, &mut trailer, at);
        if tables[0] != crcs || tables[1] != crcs || trailer != layout.trailer() {
            let table: Vec<u8> = crcs.iter().flat_map(|crc| crc.to_be_bytes()).collect();
            let mut out = file;
            out.seek(SeekFrom::Start(layout.table_at()))?;
            out.write_all(&table)?;
            out.write_all(&table)?;
            out.write_all(&layout.trailer())?;
        }
        file.sync_all()?;
    }
    Ok(checked)
}

/// Read what there is of `buf` at `at`, up to an error or the end.
fn read_some(file: &File, buf: &mut [u8], at: u64) -> usize {
    let mut done = 0;
    while done < buf.len() {
        match io::read_at(file, &mut buf[done..], at + done as u64) {
            Ok(0) | Err(_) => break,
            Ok(n) => done += n,
        }
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A segment of `len` bytes of noise, sealed.
    fn sealed(len: usize, parity: u8) -> (tempfile::NamedTempFile, Vec<u8>, Layout) {
        let mut state = 0x9E3779B97F4A7C15u64;
        let data: Vec<u8> = (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect();
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), &data).unwrap();
        let handle = File::options()
            .read(true)
            .write(true)
            .open(file.path())
            .unwrap();
        seal(&handle, len as u64, parity, &mut |_| {}).unwrap();
        let layout = Layout::new(len as u64, parity);
        assert_eq!(
            std::fs::metadata(file.path()).unwrap().len(),
            len as u64 + layout.footer_len()
        );
        (file, data, layout)
    }

    fn damage(path: &std::path::Path, at: u64, len: usize) {
        let mut bytes = std::fs::read(path).unwrap();
        for byte in &mut bytes[at as usize..at as usize + len] {
            *byte ^= 0x5A;
        }
        std::fs::write(path, bytes).unwrap();
    }

    fn check_file(path: &std::path::Path, layout: &Layout, repair: bool) -> Checked {
        let file = File::options().read(true).write(true).open(path).unwrap();
        check(&file, layout, repair, &mut |_| {}).unwrap()
    }

    #[test]
    fn a_whole_segment_checks_clean() {
        let (file, _, layout) = sealed(3_000_000, 16);
        let checked = check_file(file.path(), &layout, true);
        assert_eq!((checked.damaged, checked.lost.len()), (0, 0));
    }

    #[test]
    fn scattered_damage_and_a_burst_are_repaired() {
        let (file, data, layout) = sealed(3_000_000 + 123, 16);
        let before = std::fs::read(file.path()).unwrap();
        // Bad sectors everywhere, the last short block, a parity block, a
        // table entry, the trailer, and a burst of 16 shards
        for at in (0..3_000_000).step_by(70_001) {
            damage(file.path(), at, 1);
        }
        damage(file.path(), 3_000_100, 20);
        damage(file.path(), layout.data_len + 5 * layout.shard + 77, 3);
        damage(file.path(), layout.table_at() + 8, 4);
        damage(file.path(), before.len() as u64 - 10, 1);
        let checked = check_file(file.path(), &layout, false);
        assert!(checked.damaged > 40 && !checked.lost.is_empty());
        assert_eq!(checked.repaired, 0);
        let checked = check_file(file.path(), &layout, true);
        assert!(checked.lost.is_empty() && checked.repaired == checked.damaged);
        assert!(std::fs::read(file.path()).unwrap() == before);

        damage(file.path(), layout.shard, 16 * layout.shard as usize);
        let checked = check_file(file.path(), &layout, true);
        assert!(checked.lost.is_empty(), "{:?}", checked.lost.len());
        assert!(std::fs::read(file.path()).unwrap()[..data.len()] == data);
    }

    #[test]
    fn a_row_past_repair_is_lost_and_left_alone() {
        let (file, _, layout) = sealed(2_000_000, 8);
        // Nine blocks of one row, with eight parity shards
        for shard in 0..9 {
            damage(file.path(), shard * layout.shard + 2 * BLOCK + 5, 1);
        }
        let damaged = std::fs::read(file.path()).unwrap();
        let checked = check_file(file.path(), &layout, true);
        assert_eq!((checked.damaged, checked.lost.len()), (9, 9));
        assert!(std::fs::read(file.path()).unwrap() == damaged);
    }

    #[test]
    fn a_segment_cut_short_is_rebuilt() {
        let (file, _, layout) = sealed(1_000_000, 16);
        let whole = std::fs::read(file.path()).unwrap();
        // Its CRC tables and trailer go too: the second copy is cut
        std::fs::write(
            file.path(),
            &whole[..(layout.table_at() + layout.table_len()) as usize],
        )
        .unwrap();
        let checked = check_file(file.path(), &layout, true);
        assert!(checked.lost.is_empty());
        assert!(std::fs::read(file.path()).unwrap() == whole);
    }
}

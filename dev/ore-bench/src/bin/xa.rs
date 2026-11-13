//! Lossless coding of XA ADPCM sectors for the ORE study.
//!
//! `xa <DUMP>...`: each dump `cd xa` wrote, a sector's subheader then its
//! 2324 bytes of data, cut in groups of whole sectors, 1 MiB of data as ORE
//! groups them, each group compressed on its own by Zstandard 19, LZMA2
//! preset 9, and ore-rs's `xa` codec, whose output is decoded again and
//! compared; `best` is what ore-rs stores, Zstandard for a group a quick
//! Zstandard does better on. A row a dump: sizes and seconds on every core.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Result, bail};
use lzma_rust2::{Lzma2Options, Lzma2Writer};
use rayon::prelude::*;

#[allow(dead_code)]
#[path = "../../../../crates/ore-rs/src/range.rs"]
mod range;
#[allow(dead_code)]
#[path = "../../../../crates/ore-rs/src/xa.rs"]
mod xa;

const DATA: usize = 2324;
const GROUP: usize = 1 << 20;

fn zstd(data: &[u8]) -> u64 {
    zstd::bulk::compress(data, 19).unwrap().len() as u64
}

fn lzma2(data: &[u8]) -> u64 {
    let mut options = Lzma2Options::with_preset(9);
    options.lzma_options.dict_size = 4 << 20;
    let mut writer = Lzma2Writer::new(Vec::new(), options);
    std::io::Write::write_all(&mut writer, data).unwrap();
    writer.finish().unwrap().len() as u64
}

/// The sectors' data, without their subheaders, as ORE stores them now.
fn data(records: &[u8]) -> Vec<u8> {
    records
        .chunks(xa::RECORD)
        .flat_map(|r| &r[4..])
        .copied()
        .collect()
}

fn timed(f: impl FnOnce() -> u64) -> (u64, f64) {
    let start = Instant::now();
    let size = f();
    (size, start.elapsed().as_secs_f64())
}

fn main() -> Result<()> {
    let dumps: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if dumps.is_empty() {
        bail!("usage: xa <DUMP>...");
    }
    println!(
        "dump\tsectors\tstereo\tbytes\tzstd\tlzma2\txa\tbest\tzstd_s\tlzma2_s\txa_encode_s\txa_decode_s"
    );
    for dump in &dumps {
        let records = std::fs::read(dump)?;
        if records.is_empty() {
            continue;
        }
        let name = dump.file_stem().unwrap_or_default().to_string_lossy();
        let sectors = records.len() / xa::RECORD;
        let stereo = records.chunks(xa::RECORD).filter(|r| r[3] & 3 == 1).count();
        let groups: Vec<&[u8]> = records.chunks(GROUP / DATA * xa::RECORD).collect();
        let (z, z_s) = timed(|| groups.par_iter().map(|g| zstd(&data(g))).sum());
        let (l, l_s) = timed(|| groups.par_iter().map(|g| lzma2(&data(g))).sum());
        let start = Instant::now();
        let coded: Vec<Vec<u8>> = groups.par_iter().map(|g| xa::encode(g)).collect();
        // As ore-rs stores a group: zstd instead when a quick zstd beats xa
        let best: u64 = groups
            .par_iter()
            .zip(&coded)
            .map(|(g, c)| {
                if zstd::bulk::compress(g, 3).unwrap().len() < c.len() {
                    zstd(g)
                } else {
                    c.len() as u64
                }
            })
            .sum();
        let encode_s = start.elapsed().as_secs_f64();
        let start = Instant::now();
        groups.par_iter().zip(&coded).for_each(|(group, coded)| {
            assert!(
                xa::decode(coded, group.len()) == *group,
                "{name} does not round-trip"
            );
        });
        let decode_s = start.elapsed().as_secs_f64();
        let x: u64 = coded.iter().map(|c| c.len() as u64).sum();
        println!(
            "{name}\t{sectors}\t{stereo}\t{}\t{z}\t{l}\t{x}\t{best}\t{z_s:.1}\t{l_s:.1}\t{encode_s:.1}\t{decode_s:.1}",
            sectors * DATA
        );
    }
    Ok(())
}

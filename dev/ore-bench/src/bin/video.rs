//! Lossless coding of STR video sectors for the ORE study, by ore-rs's
//! `str` codec.
//!
//! `video <DUMP>...`: each dump `cd str` wrote with STR_DUMP set, a video
//! sector's data each, 2048 bytes, in disc order. Cut into chunks before
//! each frame, 64 sectors at most, and the chunks into groups of 1 MiB,
//! as ore-rs's writer does; each group coded by Zstandard 19 and by `str`,
//! whose output is decoded again and compared; `best` is what ore-rs
//! stores, Zstandard for a group a quick Zstandard does better on. A row a
//! dump: sizes and seconds on every core. `GROUP` sets the group size,
//! and `LZMA2` adds LZMA2 preset 9 at it.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Result, bail};
use lzma_rust2::{Lzma2Options, Lzma2Writer};
use rayon::prelude::*;

#[allow(dead_code)]
#[path = "../../../../crates/ore-rs/src/range.rs"]
mod range;
#[allow(dead_code)]
#[path = "../../../../crates/ore-rs/src/video.rs"]
mod video;

const CHUNK: usize = 64;

fn zstd(data: &[u8]) -> u64 {
    zstd::bulk::compress(data, 19).unwrap().len() as u64
}

fn lzma2(data: &[u8], group: usize) -> u64 {
    let mut options = Lzma2Options::with_preset(9);
    options.lzma_options.dict_size = group as u32;
    let mut writer = Lzma2Writer::new(Vec::new(), options);
    std::io::Write::write_all(&mut writer, data).unwrap();
    writer.finish().unwrap().len() as u64
}

/// Groups as ore-rs's writer and packer make them.
fn groups(dump: &[u8], group: usize) -> Vec<Vec<u8>> {
    let mut groups = vec![Vec::new()];
    let mut chunk: Vec<u8> = Vec::new();
    let close = |chunk: &mut Vec<u8>, groups: &mut Vec<Vec<u8>>| {
        if groups.last().unwrap().len() + chunk.len() > group {
            groups.push(Vec::new());
        }
        groups.last_mut().unwrap().append(chunk);
    };
    for sector in dump.chunks(video::SECTOR) {
        if !chunk.is_empty()
            && (video::starts_frame(sector) || chunk.len() == CHUNK * video::SECTOR)
        {
            close(&mut chunk, &mut groups);
        }
        chunk.extend_from_slice(sector);
    }
    close(&mut chunk, &mut groups);
    groups
}

fn main() -> Result<()> {
    let dumps: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if dumps.is_empty() {
        bail!("usage: video <DUMP>...");
    }
    // 1 MiB as `stream`, 4 MiB as `archive`
    let group: usize = std::env::var("GROUP").map_or(1 << 20, |g| g.parse().unwrap());
    println!("dump\tsectors\tbytes\tzstd\tstr\tbest\tzstd_s\tstr_encode_s\tstr_decode_s\tlzma2");
    for dump in &dumps {
        let bytes = std::fs::read(dump)?;
        if bytes.is_empty() {
            continue;
        }
        let name = dump.file_stem().unwrap_or_default().to_string_lossy();
        let groups = groups(&bytes, group);
        let start = Instant::now();
        let z: u64 = groups.par_iter().map(|g| zstd(g)).sum();
        let zstd_s = start.elapsed().as_secs_f64();
        let start = Instant::now();
        let coded: Vec<Vec<u8>> = groups.par_iter().map(|g| video::encode(g)).collect();
        let encode_s = start.elapsed().as_secs_f64();
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
        let start = Instant::now();
        groups.par_iter().zip(&coded).for_each(|(group, coded)| {
            assert!(
                video::decode(coded, group.len()) == *group,
                "{name} does not round-trip"
            );
        });
        let decode_s = start.elapsed().as_secs_f64();
        let s: u64 = coded.iter().map(|c| c.len() as u64).sum();
        let l: u64 = if std::env::var_os("LZMA2").is_some() {
            groups.par_iter().map(|g| lzma2(g, group)).sum()
        } else {
            0
        };
        println!(
            "{name}\t{}\t{}\t{z}\t{s}\t{best}\t{zstd_s:.1}\t{encode_s:.1}\t{decode_s:.1}\t{l}",
            bytes.len() / video::SECTOR,
            bytes.len()
        );
    }
    Ok(())
}

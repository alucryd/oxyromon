//! `solid <DISC>...` prints, tab-separated, the discs as solid streams: each
//! alone, all together in the order given, and all deduped into 16 KiB
//! FastCDC chunks then together. `solid groups [-g SIZE:CODEC,...] <DISC>...`
//! prints instead what 16 KiB chunks deduped then stored in groups weigh,
//! each disc alone and all together: groups of 256 KiB, 1 MiB and 4 MiB with
//! Zstandard 19 and LZMA2, unless `-g` names others (`1M:zstd19,4M:lzma2`).
//! Sizes in bytes. See `ore_bench::solid`.

use std::path::Path;

use anyhow::{Context, Result};
use ore_bench::solid::{Grouping, alone, deduped, grouped, together};

fn main() -> Result<()> {
    let mut paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.first().map(String::as_str) == Some("groups") {
        paths.remove(0);
        // `-g SIZE:CODEC,...`, every size and codec unless given
        let mut groupings = Vec::new();
        if paths.first().map(String::as_str) == Some("-g") {
            paths.remove(0);
            for item in paths.remove(0).split(',') {
                let (size, codec) = item.split_once(':').context("-g takes SIZE:CODEC,...")?;
                let size = match size.strip_suffix('M') {
                    Some(n) => n.parse::<usize>()? << 20,
                    None => {
                        size.strip_suffix('K')
                            .context("a size ends in K or M")?
                            .parse::<usize>()?
                            << 10
                    }
                };
                let codec = ["zstd19", "lzma2"]
                    .into_iter()
                    .find(|&c| c == codec)
                    .context("codec: zstd19 or lzma2")?;
                groupings.push(Grouping { group: size, codec });
            }
        } else {
            for codec in ["zstd19", "lzma2"] {
                for group in [256 << 10, 1 << 20, 4 << 20] {
                    groupings.push(Grouping { group, codec });
                }
            }
        }
        let discs: Vec<&Path> = paths.iter().map(Path::new).collect();
        println!("codec\tgroup\tper_disc\tlibrary");
        for (grouping, (per_disc, library)) in
            groupings.iter().zip(grouped(&discs, 16 << 10, &groupings)?)
        {
            println!(
                "{}\t{}\t{per_disc}\t{library}",
                grouping.codec, grouping.group
            );
        }
        return Ok(());
    }
    let discs: Vec<&Path> = paths.iter().map(Path::new).collect();
    println!("stream\tbytes");
    let alone = alone(&discs)?;
    for (disc, size) in discs.iter().zip(&alone) {
        println!(
            "alone {}\t{size}",
            disc.file_name().unwrap_or_default().to_string_lossy()
        );
    }
    println!("alone, sum\t{}", alone.iter().sum::<u64>());
    eprintln!("together");
    println!("together\t{}", together(&discs)?);
    eprintln!("deduped");
    let (size, references, stored) = deduped(&discs, 16 << 10)?;
    println!("cdc-16K deduped, then together\t{size}\t({references} chunks, {stored} stored)");
    Ok(())
}

//! `solid <DISC>...` prints, tab-separated, the discs as solid streams: each
//! alone, all together in the order given, and all deduped into 16 KiB
//! FastCDC chunks then together. `solid groups <DISC>...` prints instead
//! what 16 KiB chunks deduped then stored in groups of 256 KiB, 1 MiB and
//! 4 MiB weigh, with Zstandard 19 and LZMA2, each disc alone and all
//! together. Sizes in bytes. See `ore_bench::solid`.

use std::path::Path;

use anyhow::Result;
use ore_bench::solid::{Grouping, alone, deduped, grouped, together};

fn main() -> Result<()> {
    let mut paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.first().map(String::as_str) == Some("groups") {
        paths.remove(0);
        let discs: Vec<&Path> = paths.iter().map(Path::new).collect();
        let mut groupings = Vec::new();
        for codec in ["zstd19", "lzma2"] {
            for group in [256 << 10, 1 << 20, 4 << 20] {
                groupings.push(Grouping { group, codec });
            }
        }
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

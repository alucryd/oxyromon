//! `solid <DISC>...` prints, tab-separated, the discs as solid streams: each
//! alone, all together in the order given, and all deduped into 16 KiB
//! FastCDC chunks then together. Sizes in bytes. See `ore_bench::solid`.

use std::path::Path;

use anyhow::Result;
use ore_bench::solid::{alone, deduped, together};

fn main() -> Result<()> {
    let paths: Vec<String> = std::env::args().skip(1).collect();
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

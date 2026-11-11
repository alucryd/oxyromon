//! `inventory <DISC>...`
//!
//! Prints a tab-separated row per file of each GameCube disc on stdout, and
//! a row per part of its system area outside the file system: sizes in
//! bytes, compressed as Zstandard level 19 in 2 MiB pieces.

use anyhow::{Context, Result};
use ore_bench::Disc;
use ore_bench::inventory::inventory;

fn main() -> Result<()> {
    println!("disc\tdisc_size\tname\tkind\toffset\tsize\tzstd19\tsha1");
    for path in std::env::args().skip(1) {
        let name = std::path::Path::new(&path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        eprintln!("{name}");
        let mut disc =
            Disc::load(path.as_ref()).with_context(|| format!("Failed to read \"{path}\""))?;
        let entries = match inventory(&mut disc) {
            Ok(entries) => entries,
            Err(error) => {
                eprintln!("skipping \"{name}\": {error}");
                continue;
            }
        };
        for entry in entries {
            println!(
                "{name}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                disc.0.len(),
                entry.name,
                entry.kind,
                entry.offset,
                entry.size,
                entry.compressed,
                entry.sha1
            );
        }
    }
    Ok(())
}

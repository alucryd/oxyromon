//! `ore-bench -o SCRATCH [-l LEVELS] [-c CHUNKS] <DISC>...`
//!
//! Prints a tab-separated row per disc, level and chunk size on stdout, sizes
//! in bytes. `LEVELS` are Zstandard levels and `CHUNKS` chunk sizes in KiB,
//! each comma-separated. The RVZ it writes goes to `SCRATCH`, and is removed
//! once measured.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use ore_bench::{Disc, Layout, measure};
use sha1::{Digest, Sha1};

fn list<T: std::str::FromStr>(value: Option<String>, name: &str) -> Result<Vec<T>> {
    value
        .with_context(|| format!("{name} needs a value"))?
        .split(',')
        .map(|item| {
            item.parse()
                .ok()
                .with_context(|| format!("bad {name}: {item}"))
        })
        .collect()
}

fn main() -> Result<()> {
    let mut levels = vec![5, 19];
    let mut chunks = vec![32, 128, 512, 2048];
    let mut scratch = None;
    let mut discs = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-l" => levels = list(args.next(), "-l")?,
            "-c" => chunks = list(args.next(), "-c")?,
            "-o" => scratch = args.next().map(PathBuf::from),
            _ => discs.push(PathBuf::from(arg)),
        }
    }
    let Some(scratch) = scratch else {
        bail!("usage: ore-bench -o SCRATCH [-l LEVELS] [-c CHUNKS] <DISC>...");
    };

    println!(
        "disc\tsha1\tsize\tfiller\tdol\tlevel\tchunk\trvz\trvz_seconds\tplain\tdedup\tfiller_seed\tbcj"
    );
    for path in discs {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let mut disc =
            Disc::load(&path).with_context(|| format!("Failed to read \"{}\"", path.display()))?;
        let layout = match Layout::of(&mut disc) {
            Ok(layout) => layout,
            Err(error) => {
                eprintln!("skipping \"{name}\": {error}");
                continue;
            }
        };
        let sha1 = Sha1::digest(&disc.0)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let dol = layout.dol.map_or(0, |(start, end)| end - start);
        let output = scratch.join(format!("{name}.b5.rvz"));
        for &level in &levels {
            for &chunk in &chunks {
                eprintln!("{name}: level {level}, {chunk} KiB");
                let row = measure(&path, &output, &disc, &layout, level, chunk * 1024)?;
                println!(
                    "{name}\t{sha1}\t{}\t{}\t{dol}\t{}\t{}\t{}\t{:.1}\t{}\t{}\t{}\t{}",
                    disc.0.len(),
                    layout.filler_bytes(),
                    row.level,
                    row.chunk_size,
                    row.rvz,
                    row.rvz_seconds,
                    row.plain,
                    row.dedup,
                    row.filler,
                    row.bcj,
                );
            }
        }
    }
    Ok(())
}

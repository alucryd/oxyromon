//! `dedup units <DISC>...` prints a tab-separated row per unit of each
//! GameCube disc under each scheme; `dedup sum [ROWS]` adds rows up, from
//! ROWS or stdin, into a table per scheme. See `ore_bench::dedup`.

use std::io::BufReader;

use anyhow::{Context, Result, bail};
use ore_bench::Disc;
use ore_bench::dedup::{REFERENCE, STORED, sum, units};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("units") => {
            println!("disc\tscheme\tdigest\tlength\tcompressed");
            for path in args {
                let name = std::path::Path::new(&path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                eprintln!("{name}");
                let mut disc = Disc::load(path.as_ref())
                    .with_context(|| format!("Failed to read \"{path}\""))?;
                for (scheme, units) in units(&mut disc)? {
                    for unit in units {
                        println!(
                            "{name}\t{scheme}\t{:016x}\t{}\t{}",
                            unit.digest, unit.length, unit.compressed
                        );
                    }
                }
            }
        }
        Some("sum") => {
            let totals = match args.next() {
                Some(path) => sum(BufReader::new(std::fs::File::open(path)?))?,
                None => sum(std::io::stdin().lock())?,
            };
            println!(
                "scheme\tbytes\tunits\tstored\tper_disc\tlibrary\t(index: {REFERENCE} B a unit, {STORED} B a unit stored)"
            );
            for (scheme, total) in totals {
                println!(
                    "{scheme}\t{}\t{}\t{}\t{}\t{}",
                    total.length, total.references, total.stored, total.per_disc, total.library
                );
            }
        }
        _ => bail!("usage: dedup units <DISC>... | dedup sum [ROWS]"),
    }
    Ok(())
}

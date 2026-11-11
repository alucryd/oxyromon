//! `dedup units [-s SCHEMES] [-c CODECS] <DISC>...` prints a tab-separated
//! row per unit of each disc under each scheme and codec: every scheme unless
//! SCHEMES, a comma-separated list, says otherwise, and `zstd19` unless
//! CODECS does. A Wii disc's rows are named after
//! the disc and its part, as `<disc>#game0`. `dedup sum [ROWS]` adds rows up,
//! from ROWS or stdin, into a table per scheme. See `ore_bench::dedup`.

use std::io::BufReader;
use std::path::Path;

use anyhow::{Context, Result, bail};
use gcwii::disc::Source;
use ore_bench::dedup::{CODECS, REFERENCE, SCHEMES, STORED, Unit, sum, units, wii_units};
use ore_bench::{Disc, RvzDisc};

fn print(name: &str, units: Vec<(&str, Vec<Unit>)>, codecs: &[&str]) {
    for (scheme, units) in units {
        for unit in units {
            for (codec, compressed) in codecs.iter().zip(&unit.compressed) {
                // A solid unit is only compressed one way
                if scheme == "solid" && *codec != "zstd19" {
                    continue;
                }
                let scheme = match *codec {
                    "zstd19" => scheme.to_string(),
                    codec => format!("{scheme}/{codec}"),
                };
                println!(
                    "{name}\t{scheme}\t{:016x}\t{}\t{compressed}",
                    unit.digest, unit.length
                );
            }
        }
    }
}

fn list(value: Option<String>, known: &[&'static str], name: &str) -> Result<Vec<&'static str>> {
    value
        .with_context(|| format!("{name} needs a value"))?
        .split(',')
        .map(|item| {
            known
                .iter()
                .copied()
                .find(|&known| known == item)
                .with_context(|| format!("unknown {name}: {item}"))
        })
        .collect()
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1).peekable();
    match args.next().as_deref() {
        Some("units") => {
            let mut schemes: Vec<&'static str> = SCHEMES.to_vec();
            let mut codecs: Vec<&'static str> = vec!["zstd19"];
            while let Some(option) = args.next_if(|arg| arg == "-s" || arg == "-c") {
                match option.as_str() {
                    "-s" => schemes = list(args.next(), SCHEMES, "scheme")?,
                    _ => codecs = list(args.next(), CODECS, "codec")?,
                }
            }
            println!("disc\tscheme\tdigest\tlength\tcompressed");
            for path in args {
                let path = Path::new(&path);
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                eprintln!("{name}");
                let wii = rvz_rs::is_rvz(path)? && {
                    let mut disc = RvzDisc::open(path)?;
                    let mut magic = [0; 4];
                    disc.read_at(0x18, &mut magic)?;
                    magic == 0x5D1C9EA3u32.to_be_bytes()
                };
                if wii {
                    let mut disc = RvzDisc::open(path)?;
                    let (parts, left_out) = wii_units(&mut disc, &schemes, &codecs)?;
                    eprintln!("{name}: {left_out} bytes outside every partition left out");
                    for (part, units) in parts {
                        print(&format!("{name}#{part}"), units, &codecs);
                    }
                } else {
                    let mut disc = Disc::load(path)
                        .with_context(|| format!("Failed to read \"{}\"", path.display()))?;
                    print(&name, units(&mut disc, &schemes, &codecs)?, &codecs);
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
                if total.references > 0 {
                    println!(
                        "{scheme}\t{}\t{}\t{}\t{}\t{}",
                        total.length, total.references, total.stored, total.per_disc, total.library
                    );
                }
            }
        }
        _ => bail!("usage: dedup units [-s SCHEMES] <DISC>... | dedup sum [ROWS]"),
    }
    Ok(())
}

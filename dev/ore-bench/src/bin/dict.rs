//! `dict sample SAMPLES <DISC>` takes a disc's samples into the directory
//! SAMPLES; `dict train SAMPLES DICTIONARIES` trains every dictionary on
//! them; `dict units DICTIONARIES <DISC>` prints a row per chunk and
//! dictionary, as `dedup units` does, for `dedup sum`. See `ore_bench::dict`.

use std::path::Path;

use anyhow::{Result, bail};
use ore_bench::dict::{sample, train, units};

fn name(disc: &Path) -> String {
    disc.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["sample", samples, disc] => {
            let disc = Path::new(disc);
            std::fs::create_dir_all(samples)?;
            let (count, bytes) = sample(
                disc,
                &Path::new(samples).join(format!("{}.samples", name(disc))),
            )?;
            println!("disc\tsamples\tbytes");
            println!("{}\t{count}\t{bytes}", name(disc));
        }
        ["train", samples, dictionaries] => train(Path::new(samples), Path::new(dictionaries))?,
        ["units", dictionaries, disc] => {
            let disc = Path::new(disc);
            println!("disc\tscheme\tdigest\tlength\tcompressed");
            units(Path::new(dictionaries), disc, &name(disc))?;
        }
        _ => bail!(
            "usage: dict sample SAMPLES <DISC> | dict train SAMPLES DICTIONARIES | dict units DICTIONARIES <DISC>"
        ),
    }
    Ok(())
}

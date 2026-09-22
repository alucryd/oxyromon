//! `xsors`: CSO and ZSO compression and decompression, from the command line.

mod ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgMatches, Command, value_parser};
use xso_rs::{CompressOptions, DecompressOptions, Format};

fn cli() -> Command {
    Command::new("xsors")
        .version(env!("CARGO_PKG_VERSION"))
        .about("CSO/ZSO compression and decompression: ISOs are compressed, CSOs and ZSOs decompressed")
        .after_help(format!(
            "CSO blocks are compressed with zlib, ZSO blocks with LZ4 HC.\n\n{}",
            ui::AFTER_HELP
        ))
        .arg(
            Arg::new("INPUTS")
                .help("ISOs to compress, CSOs or ZSOs to decompress")
                .required(true)
                .num_args(1..)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .help("Output directory [default: next to each input]")
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("format")
                .short('f')
                .long("format")
                .help("What to compress ISOs to")
                .value_parser(["cso", "zso"])
                .default_value("cso"),
        )
        .arg(
            Arg::new("block")
                .short('b')
                .long("block")
                .help("Block size, a power of two in 2048..=262144 [default: 8192 for CSO, 16384 from 2 GiB; 2048 for ZSO]")
                .value_parser(value_parser!(u32)),
        )
        .arg(
            Arg::new("threads")
                .short('j')
                .long("threads")
                .help("Worker threads, 0 for one per core")
                .value_parser(value_parser!(usize))
                .default_value("0"),
        )
}

fn main() -> ExitCode {
    let matches = cli().get_matches();
    ui::run_all(matches.get_many::<PathBuf>("INPUTS").unwrap(), |input| {
        convert(input, &matches)
    })
}

/// Decompress `input` if it is a CSO or ZSO, compress it otherwise.
fn convert(input: &Path, matches: &ArgMatches) -> Result<String, String> {
    let dir = ui::output_dir(input, matches.get_one("output"))?;
    let threads = *matches.get_one::<usize>("threads").unwrap();
    let size = std::fs::metadata(input).map_err(|e| e.to_string())?.len();

    if matches!(ui::extension(input).as_str(), "cso" | "zso") {
        let output = ui::output_path(&dir, input, "iso");
        let bar = ui::progress_bar(size, "Decompressing", input);
        let result = xso_rs::decompress(input, &output, &DecompressOptions { threads }, &mut |n| {
            bar.inc(n)
        });
        bar.finish_and_clear();
        result.map_err(|e| e.to_string())?;
        return Ok(ui::wrote(&output));
    }

    let format = match matches.get_one::<String>("format").unwrap().as_str() {
        "zso" => Format::Zso,
        _ => Format::Cso,
    };
    let output = ui::output_path(&dir, input, format.extension());
    let options = CompressOptions {
        block_size: matches.get_one::<u32>("block").copied(),
        threads,
        ..CompressOptions::new(format)
    };
    let bar = ui::progress_bar(size, "Compressing", input);
    let result = xso_rs::compress(input, &output, &options, &mut |n| bar.inc(n));
    bar.finish_and_clear();
    let stats = result.map_err(|e| e.to_string())?;
    // How big the output is next to the original.
    Ok(format!(
        "{} ({:.1}%)",
        ui::wrote(&output),
        stats.ratio_percent()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_is_well_formed() {
        cli().debug_assert();
    }

    #[test]
    fn takes_several_inputs_and_defaults_to_cso() {
        let matches = cli()
            .try_get_matches_from(["xsors", "a.iso", "b.cso"])
            .unwrap();
        assert_eq!(matches.get_many::<PathBuf>("INPUTS").unwrap().count(), 2);
        assert_eq!(matches.get_one::<String>("format").unwrap(), "cso");
        assert!(matches.get_one::<PathBuf>("output").is_none());
    }
}

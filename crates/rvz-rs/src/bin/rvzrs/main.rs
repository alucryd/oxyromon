//! `rvzrs`: RVZ compression and decompression, from the command line.

mod ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};
use rvz_rs::{Compression, Options};

fn cli() -> Command {
    Command::new("rvzrs")
        .version(env!("CARGO_PKG_VERSION"))
        .about("RVZ compression and decompression: GameCube and Wii ISOs are compressed, RVZs decompressed")
        .after_help(format!(
            "An RVZ given --format rvz is rewritten, to recompress or scrub it. Writing an RVZ from a Wii ISO needs the Wii common key among the keys; rvzrs ships none.\n\n{}",
            ui::AFTER_HELP
        ))
        .arg(
            Arg::new("INPUTS")
                .help("ISOs to compress, RVZs to decompress")
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
                .help("What to write [default: RVZ for an ISO, ISO for an RVZ]")
                .value_parser(["iso", "rvz"]),
        )
        .arg(
            Arg::new("compression")
                .short('c')
                .long("compression")
                .help("How to compress an RVZ")
                .value_parser(["none", "bzip2", "lzma", "lzma2", "zstd"])
                .default_value("zstd"),
        )
        .arg(
            Arg::new("level")
                .short('l')
                .long("level")
                .help("Compression level: 1..=22 for zstd, 1..=9 for the others")
                .value_parser(value_parser!(i32))
                .default_value("5"),
        )
        .arg(
            Arg::new("block")
                .short('b')
                .long("block")
                .help("Block size in bytes: a power of two from 32 KiB to 2 MiB, or a multiple of 2 MiB up to 32 MiB")
                .value_parser(value_parser!(u32))
                .default_value("131072"),
        )
        .arg(
            Arg::new("scrub")
                .long("scrub")
                .help("Zero what nothing on the disc points to: smaller, but no longer the original disc")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("keys")
                .long("keys")
                .help("Keys file holding the Wii common key: text, a BootMii keys.bin, or an otp.bin")
                .value_parser(value_parser!(PathBuf)),
        )
}

fn main() -> ExitCode {
    ui::run(cli(), convert)
}

/// Decompress `input` if it is an RVZ, compress it otherwise.
fn convert(
    input: &Path,
    matches: &ArgMatches,
    outputs: &mut ui::Outputs,
) -> Result<String, String> {
    let size = std::fs::metadata(input).map_err(|e| e.to_string())?.len();
    let dir = ui::output_dir(input, matches)?;
    let is_rvz = rvz_rs::is_rvz(input).map_err(|e| e.to_string())?;
    let to = match matches.get_one::<String>("format").map(String::as_str) {
        Some(format) => format,
        None if is_rvz => "iso",
        None => "rvz",
    };

    if to == "iso" {
        if !is_rvz {
            return Err("not an RVZ".into());
        }
        let output = outputs.claim(ui::output_path(&dir, input, "iso"))?;
        let bar = ui::progress_bar(size, "Decompressing", input);
        let result = rvz_rs::decompress(input, &output, &mut |n| bar.inc(n));
        bar.finish_and_clear();
        result.map_err(|e| e.to_string())?;
        return Ok(ui::wrote(&output));
    }

    let compression = match matches.get_one::<String>("compression").unwrap().as_str() {
        "none" => Compression::None,
        "bzip2" => Compression::Bzip2,
        "lzma" => Compression::Lzma,
        "lzma2" => Compression::Lzma2,
        _ => Compression::Zstd,
    };
    let options = Options {
        compression,
        level: *matches.get_one::<i32>("level").unwrap(),
        chunk_size: *matches.get_one::<u32>("block").unwrap(),
        scrub: matches.get_flag("scrub"),
    };
    let keys = match matches.get_one::<PathBuf>("keys") {
        Some(path) => rvz_rs::load_keys(path).map_err(|e| format!("{}: {e}", path.display()))?,
        None => Vec::new(),
    };
    let output = outputs.claim(ui::output_path(&dir, input, "rvz"))?;
    let bar = ui::progress_bar(size, "Compressing", input);
    let result = rvz_rs::compress(input, &output, &options, &keys, &mut |n| bar.inc(n));
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
    fn takes_several_inputs_and_defaults_to_zstd() {
        let matches = cli()
            .try_get_matches_from(["rvzrs", "a.iso", "b.rvz"])
            .unwrap();
        assert_eq!(matches.get_many::<PathBuf>("INPUTS").unwrap().count(), 2);
        assert_eq!(matches.get_one::<String>("compression").unwrap(), "zstd");
        assert_eq!(*matches.get_one::<u32>("block").unwrap(), 131072);
        assert!(!matches.get_flag("scrub"));
    }
}

//! `orers`: ORE compression and decompression, from the command line.

mod ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgMatches, Command, value_parser};
use ore_rs::{Codec, Options, Profile};

fn cli() -> Command {
    Command::new("orers")
        .version(env!("CARGO_PKG_VERSION"))
        .about("ORE compression and decompression: GameCube and Wii ISOs are compressed, OREs decompressed")
        .after_help(format!(
            "Writing an ORE from a Wii ISO needs the Wii common key among the keys; orers ships none.\n\n{}",
            ui::AFTER_HELP
        ))
        .arg(
            Arg::new("INPUTS")
                .help("ISOs to compress, OREs to decompress")
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
            Arg::new("profile")
                .short('p')
                .long("profile")
                .help("stream: 1 MiB groups, quick to read; archive: 4 MiB groups, smaller")
                .value_parser(["stream", "archive"])
                .default_value("stream"),
        )
        .arg(
            Arg::new("compression")
                .short('c')
                .long("compression")
                .help("zstd: the quicker to read; lzma2: the smaller")
                .value_parser(["zstd", "lzma2"])
                .default_value("zstd"),
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

/// Decompress `input` if it is an ORE, compress it otherwise.
fn convert(
    input: &Path,
    matches: &ArgMatches,
    outputs: &mut ui::Outputs,
) -> Result<String, String> {
    let size = std::fs::metadata(input).map_err(|e| e.to_string())?.len();
    let dir = ui::output_dir(input, matches)?;
    if ore_rs::is_ore(input).map_err(|e| e.to_string())? {
        let output = outputs.claim(ui::output_path(&dir, input, "iso"))?;
        let bar = ui::progress_bar(size, "Decompressing", input);
        let result = ore_rs::decompress(input, &output, &mut |n| bar.inc(n));
        bar.finish_and_clear();
        result.map_err(|e| e.to_string())?;
        return Ok(ui::wrote(&output));
    }

    let options = Options {
        profile: match matches.get_one::<String>("profile").unwrap().as_str() {
            "archive" => Profile::Archive,
            _ => Profile::Stream,
        },
        codec: match matches.get_one::<String>("compression").unwrap().as_str() {
            "lzma2" => Codec::Lzma2,
            _ => Codec::Zstd,
        },
    };
    let keys = match matches.get_one::<PathBuf>("keys") {
        Some(path) => ore_rs::load_keys(path).map_err(|e| format!("{}: {e}", path.display()))?,
        None => Vec::new(),
    };
    let output = outputs.claim(ui::output_path(&dir, input, "ore"))?;
    let bar = ui::progress_bar(size, "Compressing", input);
    let result = ore_rs::compress(input, &output, &options, &keys, &mut |n| bar.inc(n));
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
    fn takes_several_inputs_and_defaults_to_stream() {
        let matches = cli()
            .try_get_matches_from(["orers", "a.iso", "b.ore"])
            .unwrap();
        assert_eq!(matches.get_many::<PathBuf>("INPUTS").unwrap().count(), 2);
        assert_eq!(matches.get_one::<String>("profile").unwrap(), "stream");
        assert_eq!(matches.get_one::<String>("compression").unwrap(), "zstd");
    }
}

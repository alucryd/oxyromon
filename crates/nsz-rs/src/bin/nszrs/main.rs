//! `nszrs`: NSP to NSZ compression and back, from the command line.

mod ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgAction, ArgGroup, ArgMatches, Command, value_parser};
use nsz_rs::keys::Keys;
use nsz_rs::pipeline::{Compression, compress_nsp, decompress_nsz};

fn cli() -> Command {
    Command::new("nszrs")
        .version(env!("CARGO_PKG_VERSION"))
        .about("NSP to NSZ compression and back: NSPs are compressed, NSZs decompressed")
        .after_help(format!(
            "Decompression verifies every NCA against the CNMT.\n\n{}",
            ui::AFTER_HELP
        ))
        .arg(
            Arg::new("INPUTS")
                .help("NSPs to compress, NSZs to decompress")
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
        // nsz's own flags, which the extension makes optional.
        .arg(
            Arg::new("compress")
                .short('C')
                .long("compress")
                .help("Compress every input, whatever its extension")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("decompress")
                .short('D')
                .long("decompress")
                .help("Decompress every input, whatever its extension")
                .action(ArgAction::SetTrue),
        )
        .group(ArgGroup::new("mode").args(["compress", "decompress"]))
        .arg(
            Arg::new("fix-padding")
                .short('F')
                .long("fix-padding")
                .help("Re-pad the output header to 0x20 alignment")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("long-distance")
                .short('L')
                .long("long-distance")
                .help("Enable zstd long-distance matching")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("solid")
                .short('S')
                .long("solid")
                .help("Compress as a solid stream (default)")
                .action(ArgAction::SetTrue)
                .overrides_with("block"),
        )
        .arg(
            Arg::new("block")
                .short('B')
                .long("block")
                .help("Compress as independent blocks, in parallel")
                .action(ArgAction::SetTrue)
                .overrides_with("solid"),
        )
        .arg(
            Arg::new("level")
                .short('l')
                .long("level")
                .help("zstd compression level")
                .value_parser(value_parser!(i32))
                .default_value("18"),
        )
        .arg(
            Arg::new("bs-exp")
                .short('s')
                .long("bs-exp")
                .visible_alias("block-size-exp")
                .help("Block size exponent for -B, 14..=32")
                .value_parser(value_parser!(i8))
                .default_value("20"),
        )
        .arg(
            Arg::new("keys")
                .short('k')
                .long("keys")
                .help("prod.keys path [default: ~/.switch/prod.keys]")
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("skip-key-check")
                .short('x')
                .long("skip-key-check")
                .help("Skip the CRC32 check of known keys")
                .action(ArgAction::SetTrue),
        )
        // Accepted for nsz compatibility: keeping unknown members and parsing
        // the CNMT are what nsz-rs always does.
        .arg(
            Arg::new("keep")
                .short('K')
                .long("keep")
                .action(ArgAction::SetTrue)
                .hide(true),
        )
        .arg(
            Arg::new("always-parse-cnmt")
                .short('P')
                .long("always-parse-cnmt")
                .action(ArgAction::SetTrue)
                .hide(true),
        )
}

fn default_keys_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".switch").join("prod.keys")
}

fn main() -> ExitCode {
    let matches = cli().get_matches();
    ui::run_all(matches.get_many::<PathBuf>("INPUTS").unwrap(), |input| {
        convert(input, &matches)
    })
}

/// Decompress `input` if it is an NSZ, compress it otherwise, unless `-C` or
/// `-D` says which.
fn convert(input: &Path, matches: &ArgMatches) -> Result<String, String> {
    let decompress = if matches.get_flag("compress") {
        false
    } else {
        matches.get_flag("decompress") || ui::extension(input) == "nsz"
    };
    let dir = ui::output_dir(input, matches.get_one("output"))?;
    let fix_padding = matches.get_flag("fix-padding");
    // Only loaded for files that need them: containers without NCAs don't.
    let keys_path = matches
        .get_one::<PathBuf>("keys")
        .cloned()
        .unwrap_or_else(default_keys_path);
    let verify_crc = !matches.get_flag("skip-key-check");
    let keys = || Keys::load(&keys_path, verify_crc);
    let size = std::fs::metadata(input).map_err(|e| e.to_string())?.len();

    let (output, bar, result) = if decompress {
        let output = ui::output_path(&dir, input, "nsp");
        let bar = ui::progress_bar(size, "Decompressing", input);
        // Strict: a hash mismatch is an error and the output is removed.
        let result = decompress_nsz(input, &output, keys, fix_padding, true, true, &mut |n| {
            bar.inc(n)
        })
        .map(|_| ());
        (output, bar, result)
    } else {
        let output = ui::output_path(&dir, input, "nsz");
        let compression = Compression {
            level: *matches.get_one::<i32>("level").unwrap(),
            ldm: matches.get_flag("long-distance"),
            block_size_exponent: matches
                .get_flag("block")
                .then(|| *matches.get_one::<i8>("bs-exp").unwrap()),
        };
        let bar = ui::progress_bar(size, "Compressing", input);
        let result = compress_nsp(input, &output, keys, &compression, fix_padding, &mut |n| {
            bar.inc(n)
        });
        (output, bar, result)
    };
    bar.finish_and_clear();
    result.map_err(|e| e.to_string())?;
    Ok(ui::wrote(&output))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_is_well_formed() {
        cli().debug_assert();
    }

    #[test]
    fn still_takes_the_nsz_flags() {
        for argv in [
            ["nszrs", "-D", "-F", "-o", "out", "game.nsz"].as_slice(),
            ["nszrs", "-C", "-K", "-L", "-P", "-o", "out", "game.nsp"].as_slice(),
        ] {
            cli().try_get_matches_from(argv).unwrap();
        }
    }

    #[test]
    fn needs_no_mode_but_takes_one_at_most() {
        cli().try_get_matches_from(["nszrs", "game.nsp"]).unwrap();
        assert!(
            cli()
                .try_get_matches_from(["nszrs", "-C", "-D", "game.nsp"])
                .is_err()
        );
    }
}

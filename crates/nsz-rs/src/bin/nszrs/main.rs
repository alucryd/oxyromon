//! `nszrs`: NSP to NSZ compression and back, from the command line.

mod ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};
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
        .arg(
            Arg::new("level")
                .short('l')
                .long("level")
                .help("zstd compression level")
                .value_parser(value_parser!(i32))
                .default_value("18"),
        )
        .arg(
            Arg::new("block")
                .short('b')
                .long("block")
                .help("Compress in independent blocks of this size, a power of two in 16384..=4294967296, instead of one solid stream")
                .value_parser(block_exponent),
        )
        .arg(
            Arg::new("long-distance")
                .long("long-distance")
                .help("Enable zstd long-distance matching")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("fix-padding")
                .long("fix-padding")
                .help("Re-pad the output header to 0x20 alignment")
                .action(ArgAction::SetTrue),
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
                .long("skip-key-check")
                .help("Skip the CRC32 check of known keys")
                .action(ArgAction::SetTrue),
        )
}

/// A `--block` size in bytes, as the exponent the library takes.
fn block_exponent(size: &str) -> Result<i8, String> {
    let size: u64 = size
        .parse()
        .map_err(|_| format!("`{size}` is not a size in bytes"))?;
    if !size.is_power_of_two() || !(14..=32).contains(&size.trailing_zeros()) {
        return Err("not a power of two in 16384..=4294967296".into());
    }
    Ok(size.trailing_zeros() as i8)
}

fn default_keys_path() -> Result<PathBuf, String> {
    let home = std::env::home_dir()
        .ok_or("no home directory to find .switch/prod.keys in; pass it with -k")?;
    Ok(home.join(".switch").join("prod.keys"))
}

fn main() -> ExitCode {
    ui::run(cli(), convert)
}

/// Decompress `input` if it is an NSZ, compress it otherwise.
fn convert(
    input: &Path,
    matches: &ArgMatches,
    outputs: &mut ui::Outputs,
) -> Result<String, String> {
    let size = std::fs::metadata(input).map_err(|e| e.to_string())?.len();
    let decompress = ui::extension(input) == "nsz";
    let dir = ui::output_dir(input, matches)?;
    let fix_padding = matches.get_flag("fix-padding");
    // Only loaded for files that need them: containers without NCAs don't.
    let keys_path = match matches.get_one::<PathBuf>("keys") {
        Some(path) => path.clone(),
        None => default_keys_path()?,
    };
    let verify_crc = !matches.get_flag("skip-key-check");
    let keys = || Keys::load(&keys_path, verify_crc);

    let (output, bar, result) = if decompress {
        let output = outputs.claim(ui::output_path(&dir, input, "nsp"))?;
        let bar = ui::progress_bar(size, "Decompressing", input);
        // A hash mismatch is an error, and the output is removed.
        let (verify, strict) = (true, true);
        let result = decompress_nsz(
            input,
            &output,
            keys,
            fix_padding,
            verify,
            strict,
            &mut |n| bar.inc(n),
        )
        .map(|_| ());
        (output, bar, result)
    } else {
        let output = outputs.claim(ui::output_path(&dir, input, "nsz"))?;
        let compression = Compression {
            level: *matches.get_one::<i32>("level").unwrap(),
            ldm: matches.get_flag("long-distance"),
            block_size_exponent: matches.get_one::<i8>("block").copied(),
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
    fn takes_several_inputs() {
        let matches = cli()
            .try_get_matches_from(["nszrs", "a.nsp", "b.nsz"])
            .unwrap();
        assert_eq!(matches.get_many::<PathBuf>("INPUTS").unwrap().count(), 2);
        assert!(matches.get_one::<i8>("block").is_none());
    }

    #[test]
    fn a_block_size_is_a_power_of_two_in_range() {
        assert_eq!(block_exponent("1048576"), Ok(20));
        assert_eq!(block_exponent("16384"), Ok(14));
        for bad in ["1000000", "8192", "8589934592", "1M"] {
            assert!(block_exponent(bad).is_err(), "{bad}");
        }
    }
}

//! `nszrs`: the slice of nsz that matters, as a CLI.
//!
//! Mirrors the nsz flags oxyromon used, so it can stand in for nsz there:
//!   nsz -D -F -o <dir> <file.nsz>          decompress
//!   nsz -C -K -L -P -o <dir> <file.nsp>    solid compress (keep, LDM, parse cnmt)

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgAction, ArgGroup, ArgMatches, Command, value_parser};
use indicatif::{ProgressBar, ProgressStyle};
use nsz_rs::keys::Keys;
use nsz_rs::pipeline::{Compression, compress_nsp, decompress_nsz};

fn cli() -> Command {
    Command::new("nszrs")
        .version(env!("CARGO_PKG_VERSION"))
        .about("NSP to NSZ compression and back")
        .arg(
            Arg::new("FILES")
                .help("NSPs to compress, or NSZs to decompress")
                .required(true)
                .num_args(1..)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("compress")
                .short('C')
                .long("compress")
                .help("Compress NSP -> NSZ")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("decompress")
                .short('D')
                .long("decompress")
                .help("Decompress NSZ -> NSP, verifying every NCA against the CNMT")
                .action(ArgAction::SetTrue),
        )
        .group(
            ArgGroup::new("mode")
                .args(["compress", "decompress"])
                .required(true),
        )
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
            Arg::new("output")
                .short('o')
                .long("output")
                .help("Output directory [default: next to each input]")
                .value_parser(value_parser!(PathBuf)),
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

fn out_path(input: &Path, dir: Option<&PathBuf>, new_ext: &str) -> PathBuf {
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "out".into());
    let base = match dir {
        Some(d) => d.clone(),
        None => input
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from(".")),
    };
    base.join(format!("{stem}{new_ext}"))
}

fn main() -> ExitCode {
    let matches = cli().get_matches();
    let failures = run(&matches);
    if failures > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Process every input, returning how many failed.
fn run(matches: &ArgMatches) -> usize {
    let decompress = matches.get_flag("decompress");
    let fix_padding = matches.get_flag("fix-padding");
    let output_dir = matches.get_one::<PathBuf>("output");

    // Only loaded for files that need them: containers without NCAs don't.
    let keys_path = matches
        .get_one::<PathBuf>("keys")
        .cloned()
        .unwrap_or_else(default_keys_path);
    let verify_crc = !matches.get_flag("skip-key-check");
    let keys = || Keys::load(&keys_path, verify_crc);

    // Hidden automatically when stderr isn't a terminal.
    let style = ProgressStyle::with_template(
        "{prefix:>13.bold} [{bar:30}] {percent:>3}% {binary_bytes_per_sec:>12} ETA {eta:>3} {wide_msg}",
    )
    .unwrap()
    .progress_chars("=> ");

    let mut failures = 0usize;
    for input in matches.get_many::<PathBuf>("FILES").unwrap() {
        let size = match std::fs::metadata(input) {
            Ok(m) => m.len(),
            Err(e) => {
                eprintln!("nszrs: {}: {e}", input.display());
                failures += 1;
                continue;
            }
        };
        let bar = ProgressBar::new(size).with_style(style.clone());
        bar.set_prefix(if decompress {
            "Decompressing"
        } else {
            "Compressing"
        });
        bar.set_message(
            input
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        );
        let mut progress = |n| bar.inc(n);
        let result = if decompress {
            let out = out_path(input, output_dir, ".nsp");
            // Strict: a hash mismatch is an error and the output is removed.
            decompress_nsz(input, &out, keys, fix_padding, true, true, &mut progress).map(|_| out)
        } else {
            let out = out_path(input, output_dir, ".nsz");
            let compression = Compression {
                level: *matches.get_one::<i32>("level").unwrap(),
                ldm: matches.get_flag("long-distance"),
                block_size_exponent: matches
                    .get_flag("block")
                    .then(|| *matches.get_one::<i8>("bs-exp").unwrap()),
            };
            compress_nsp(input, &out, keys, &compression, fix_padding, &mut progress).map(|()| out)
        };
        bar.finish_and_clear();
        match result {
            Ok(out) => println!("nszrs: wrote {}", out.display()),
            Err(e) => {
                eprintln!("nszrs: {}: {e}", input.display());
                failures += 1;
            }
        }
    }
    failures
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_is_well_formed() {
        cli().debug_assert();
    }

    #[test]
    fn accepts_the_flags_oxyromon_used_with_nsz() {
        for argv in [
            ["nszrs", "-D", "-F", "-o", "out", "game.nsz"].as_slice(),
            ["nszrs", "-C", "-K", "-L", "-P", "-o", "out", "game.nsp"].as_slice(),
        ] {
            cli().try_get_matches_from(argv).unwrap();
        }
    }

    #[test]
    fn needs_exactly_one_mode() {
        assert!(cli().try_get_matches_from(["nszrs", "game.nsp"]).is_err());
        assert!(
            cli()
                .try_get_matches_from(["nszrs", "-C", "-D", "game.nsp"])
                .is_err()
        );
    }
}

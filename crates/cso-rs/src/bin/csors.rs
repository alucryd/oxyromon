//! `csors`: the slice of maxcso that matters, as a CLI.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgAction, ArgGroup, ArgMatches, Command, value_parser};
use cso_rs::{CompressOptions, DecompressOptions, Format};
use indicatif::{ProgressBar, ProgressStyle};

fn cli() -> Command {
    Command::new("csors")
        .version(env!("CARGO_PKG_VERSION"))
        .about("CSO/ZSO compression and decompression")
        .after_help("CSO blocks are compressed with zlib, ZSO blocks with LZ4 HC.")
        .arg(
            Arg::new("INPUT")
                .help("Raw ISO to compress, or CSO/ZSO to decompress")
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("input")
                .short('i')
                .long("input")
                .help("Same as INPUT")
                .value_parser(value_parser!(PathBuf))
                .conflicts_with("INPUT"),
        )
        .group(ArgGroup::new("inputs").args(["INPUT", "input"]).required(true))
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .help("Output file")
                .required(true)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("compress")
                .long("compress")
                .help("Compress a raw ISO (default)")
                .action(ArgAction::SetTrue)
                .overrides_with("decompress"),
        )
        .arg(
            Arg::new("decompress")
                .long("decompress")
                .help("Decompress a CSO or ZSO")
                .action(ArgAction::SetTrue)
                .overrides_with("compress"),
        )
        .arg(
            Arg::new("format")
                .long("format")
                .help("Output format [default: from the output extension]")
                .value_parser(["cso", "cso1", "zso"]),
        )
        .arg(
            Arg::new("block")
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
    match run(&cli().get_matches()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("csors: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(matches: &ArgMatches) -> Result<(), String> {
    let input = matches
        .get_one::<PathBuf>("INPUT")
        .or_else(|| matches.get_one::<PathBuf>("input"))
        .unwrap();
    let output = matches.get_one::<PathBuf>("output").unwrap();
    let decompress = matches.get_flag("decompress");
    let threads = *matches.get_one::<usize>("threads").unwrap();

    let size = std::fs::metadata(input)
        .map_err(|e| format!("{}: {e}", input.display()))?
        .len();
    // Hidden automatically when stderr isn't a terminal.
    let bar = ProgressBar::new(size).with_style(
        ProgressStyle::with_template(
            "{prefix:>13.bold} [{bar:30}] {percent:>3}% {binary_bytes_per_sec:>12} ETA {eta:>3} {wide_msg}",
        )
        .unwrap()
        .progress_chars("=> "),
    );
    bar.set_prefix(if decompress {
        "Decompressing"
    } else {
        "Compressing"
    });
    bar.set_message(display(input));
    let mut progress = |n| bar.inc(n);

    let result = if decompress {
        cso_rs::decompress(input, output, &DecompressOptions { threads }, &mut progress)
    } else {
        let format = match matches.get_one::<String>("format").map(String::as_str) {
            Some("zso") => Format::Zso,
            Some(_) => Format::Cso,
            None => format_from_extension(output)?,
        };
        let options = CompressOptions {
            format,
            block_size: matches.get_one::<u32>("block").copied(),
            threads,
        };
        cso_rs::compress(input, output, &options, &mut progress)
    };
    bar.finish_and_clear();

    let stats = result.map_err(|e| format!("{}: {e}", input.display()))?;
    if decompress {
        println!("csors: wrote {}", output.display());
    } else {
        // How big the output is next to the original.
        println!(
            "csors: wrote {} ({:.1}%)",
            output.display(),
            stats.ratio_percent()
        );
    }
    Ok(())
}

fn format_from_extension(path: &Path) -> Result<Format, String> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("cso") => Ok(Format::Cso),
        Some("zso") => Ok(Format::Zso),
        _ => Err(format!(
            "cannot tell the format from `{}`; pass --format=cso or --format=zso",
            display(path)
        )),
    }
}

fn display(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_is_well_formed() {
        cli().debug_assert();
    }

    #[test]
    fn accepts_the_flags_oxyromon_used_with_maxcso() {
        for argv in [
            [
                "csors",
                "--block=2048",
                "--format=cso1",
                "in.iso",
                "-o",
                "out.cso",
            ]
            .as_slice(),
            [
                "csors",
                "--block=2048",
                "--format=zso",
                "in.iso",
                "-o",
                "out.zso",
            ]
            .as_slice(),
            ["csors", "--decompress", "in.cso", "-o", "out.iso"].as_slice(),
        ] {
            cli().try_get_matches_from(argv).unwrap();
        }
    }
}

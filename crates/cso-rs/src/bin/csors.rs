//! `csors`: a drop-in stand-in for the slice of maxcso that matters.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cso_rs::{CompressOptions, DecompressOptions, Error, Format, Methods};

const USAGE: &str = "\
csors - CSO/ZSO compression and decompression

USAGE:
    csors [options] INPUT -o OUTPUT

MODES:
    --compress              Compress a raw ISO (default)
    --decompress            Decompress a CSO or ZSO

FORMAT:
    --format=cso|zso        Output format. Default: from the output extension.

BLOCKS:
    --block=N               Block size, a power of two in 2048..=262144.
                            Default: 2048, or 16384 for inputs of 2 GiB or more.

METHODS (CSO stores deflate, ZSO stores lz4):
    --use-zlib / --no-zlib
    --use-zlib-brute / --no-zlib-brute
    --use-libdeflate / --no-libdeflate
    --use-zopfli / --no-zopfli
    --use-lz4 / --no-lz4
    --use-lz4-hc / --no-lz4-hc
    --use-lz4-hc-brute / --no-lz4-hc-brute

    Defaults: cso -> zlib + zlib-brute. zso -> lz4 + lz4-hc.

COST:
    --orig-max-cost=N       Percent of a block it may grow by before being stored raw.
    --lz4-max-cost=N        Percent lz4 may cost relative to deflate.

GENERAL:
    -j, --threads=N         Worker threads. Default: one per core.
    -h, --help              Show this help.
";

struct Args {
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    decompress: bool,
    format: Option<Format>,
    block: Option<u32>,
    threads: usize,
    orig_max_cost: f64,
    lz4_max_cost: f64,
    overrides: Vec<(String, bool)>,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            input: None,
            output: None,
            decompress: false,
            format: None,
            block: None,
            threads: 0,
            orig_max_cost: 0.0,
            lz4_max_cost: 0.0,
            overrides: Vec::new(),
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("csors: {msg}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args = parse(std::env::args().skip(1).collect())?;

    let input = args.input.ok_or("no input file given")?;
    let output = args.output.ok_or("no output file given (-o)")?;

    let bar = indicatif::ProgressBar::new(0);
    bar.set_style(
        indicatif::ProgressStyle::default_bar()
            .template("{wide_bar} {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("=> "),
    );

    let mut progress = |p: cso_rs::Progress| {
        bar.set_length(p.total);
        bar.set_position(p.done);
    };

    let result = if args.decompress {
        cso_rs::decompress(
            &input,
            &output,
            &DecompressOptions {
                threads: args.threads,
            },
            &mut progress,
        )
        .map_err(|e| e.to_string())
    } else {
        let format = match args.format {
            Some(f) => f,
            None => format_from_extension(&output)?,
        };
        let methods = resolve_methods(format, &args.overrides)?;
        let opts = CompressOptions {
            format,
            block_size: args.block,
            methods,
            threads: args.threads,
            orig_max_cost_percent: args.orig_max_cost,
            lz4_max_cost_percent: args.lz4_max_cost,
        };
        cso_rs::compress(&input, &output, &opts, &mut progress)
            .map_err(|e: Error| e.to_string())
    };

    bar.finish_and_clear();
    let stats = result?;
    println!(
        "{} -> {} ({:.1}%)",
        display(&input),
        display(&output),
        stats.ratio_percent()
    );
    Ok(())
}

fn format_from_extension(path: &Path) -> Result<Format, String> {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()) {
        Some(s) if s == "cso" => Ok(Format::Cso),
        Some(s) if s == "zso" => Ok(Format::Zso),
        _ => Err(format!(
            "cannot tell the format from `{}`; pass --format=cso or --format=zso",
            display(path)
        )),
    }
}

fn display(path: &Path) -> String {
    path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned()
}

/// Resolve an option's value from `--key=value` or the next argv entry.
fn value_of(argv: &[String], i: &mut usize, inline: Option<&str>) -> Result<String, String> {
    if let Some(v) = inline {
        return Ok(v.to_string());
    }
    *i += 1;
    argv.get(*i)
        .cloned()
        .ok_or_else(|| "option needs a value".to_string())
}

fn parse(argv: Vec<String>) -> Result<Args, String> {
    let mut a = Args::default();
    let mut i = 0usize;

    while i < argv.len() {
        let arg = argv[i].clone();
        if arg == "-h" || arg == "--help" {
            print!("{USAGE}");
            std::process::exit(0);
        }
        let (key, inline) = match arg.split_once('=') {
            Some((k, v)) => (k, Some(v)),
            None => (arg.as_str(), None),
        };

        match key {
            "--compress" => a.decompress = false,
            "--decompress" => a.decompress = true,
            "--format" => {
                let v = value_of(&argv, &mut i, inline)?;
                a.format = Some(match v.as_str() {
                    "cso" | "cso1" => Format::Cso,
                    "zso" => Format::Zso,
                    other => return Err(format!("unknown format `{other}`")),
                })
            }
            "--block" => {
                let v = value_of(&argv, &mut i, inline)?;
                a.block = Some(v.parse::<u32>().map_err(|_| format!("bad block size `{v}`"))?);
            }
            "--orig-max-cost" => {
                let v = value_of(&argv, &mut i, inline)?;
                a.orig_max_cost = v.parse().map_err(|_| format!("bad --orig-max-cost `{v}`"))?;
            }
            "--lz4-max-cost" => {
                let v = value_of(&argv, &mut i, inline)?;
                a.lz4_max_cost = v.parse().map_err(|_| format!("bad --lz4-max-cost `{v}`"))?;
            }
            "-j" | "--threads" => {
                let v = value_of(&argv, &mut i, inline)?;
                a.threads = v.parse().map_err(|_| format!("bad thread count `{v}`"))?;
            }
            "-o" | "--output" => a.output = Some(PathBuf::from(value_of(&argv, &mut i, inline)?)),
            "-i" | "--input" => a.input = Some(PathBuf::from(value_of(&argv, &mut i, inline)?)),
            other => {
                if let Some(name) = other.strip_prefix("--use-") {
                    a.overrides.push((name.to_string(), true));
                } else if let Some(name) = other.strip_prefix("--no-") {
                    a.overrides.push((name.to_string(), false));
                } else if other.starts_with('-') {
                    return Err(format!("unknown option `{other}`"));
                } else {
                    if a.input.is_some() {
                        return Err(format!("more than one input file: `{other}`"));
                    }
                    a.input = Some(PathBuf::from(other));
                }
            }
        }

        i += 1;
    }

    Ok(a)
}

/// Start from the format's default set and apply the `--use-`/`--no-` toggles,
/// the way maxcso does.
fn resolve_methods(format: Format, overrides: &[(String, bool)]) -> Result<Methods, String> {
    let mut m = Methods::default_for(format);
    for (name, on) in overrides {
        match name.as_str() {
            "zlib" => m.zlib = *on,
            "zlib-brute" => m.zlib_brute = *on,
            "libdeflate" => m.libdeflate = *on,
            "zopfli" => m.zopfli = *on,
            "lz4" => m.lz4 = *on,
            "lz4-hc" => m.lz4_hc = *on,
            "lz4-hc-brute" => m.lz4_hc_brute = *on,
            other => return Err(format!("unknown method `{other}`")),
        }
    }
    Ok(m)
}

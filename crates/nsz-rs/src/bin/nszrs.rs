//! `nszrs` — a parity CLI for the `nsz-rs` library.
//!
//! Mirrors the subset of the `nsz` CLI that oxyromon shells out to:
//!   nsz -D -F -o <dir> <file.nsz>          decompress
//!   nsz -C -K -L -P -o <dir> <file.nsp>    solid compress (keep, LDM, parse cnmt)
//!
//! Flags:
//!   -C / --compress            compress NSP -> NSZ
//!   -D / --decompress          decompress NSZ -> NSP
//!   -F / --fix-padding         re-pad the output header to 0x20 alignment
//!   -K / --keep                (accepted; keep is the default here)
//!   -L / --long-distance       enable zstd long-distance matching
//!   -P / --always-parse-cnmt   (accepted; cnmt is always parsed for verify)
//!   -x / --skip-key-check      skip the CRC32 check of known keys
//!   -S / --solid               solid stream (default)
//!   -B / --block               block stream (parallel)
//!   -l <N> / --level <N>       zstd compression level (default 18)
//!   -s <N> / --bs-exp <N>      block size exponent for -B (default 20)
//!   -o <dir> / --output <dir>  output directory (default: input's dir)
//!   -k <path> / --keys <path>  prod.keys path (default ~/.switch/prod.keys)
//!   -h / --help                show usage

use std::path::{Path, PathBuf};

use nsz_rs::keys::Keys;
use nsz_rs::pipeline::{compress_nsp, decompress_nsz, Compression};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Compress,
    Decompress,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stream {
    Solid,
    Block,
}

struct Args {
    mode: Mode,
    stream: Stream,
    fix_padding: bool,
    ldm: bool,
    level: i32,
    block_exp: i8,
    output_dir: Option<PathBuf>,
    keys_path: Option<PathBuf>,
    skip_key_check: bool,
    inputs: Vec<PathBuf>,
}

fn default_keys_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".switch").join("prod.keys")
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut mode: Option<Mode> = None;
    let mut stream = Stream::Solid;
    let mut fix_padding = false;
    let mut ldm = false;
    let mut level: i32 = 18;
    let mut block_exp: i8 = 20;
    let mut output_dir: Option<PathBuf> = None;
    let mut keys_path: Option<PathBuf> = None;
    let mut skip_key_check = false;
    let mut inputs: Vec<PathBuf> = Vec::new();

    let mut i = 0;
    while i < argv.len() {
        let a = argv[i].as_str();
        // Support `-l18` / `--level=18` and `-l 18` / `--level 18`.
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) => (f, Some(v.to_string())),
            None => (a, None),
        };
        let mut take_value = |name: &str| -> Result<String, String> {
            if let Some(v) = inline.clone() {
                return Ok(v);
            }
            i += 1;
            argv.get(i)
                .cloned()
                .ok_or_else(|| format!("{name} requires a value"))
        };
        match flag {
            "-C" | "--compress" => mode = Some(Mode::Compress),
            "-D" | "--decompress" => mode = Some(Mode::Decompress),
            "-F" | "--fix-padding" => fix_padding = true,
            "-K" | "--keep" => {} // accepted, no-op (keep is default)
            "-L" | "--long-distance" => ldm = true,
            "-P" | "--always-parse-cnmt" => {} // accepted, cnmt always parsed
            "-S" | "--solid" => stream = Stream::Solid,
            "-B" | "--block" => stream = Stream::Block,
            "-l" | "--level" => {
                level = take_value("-l")?
                    .parse()
                    .map_err(|_| "invalid level".to_string())?
            }
            "-s" | "--bs-exp" | "--block-size-exp" => {
                block_exp = take_value("-s")?
                    .parse()
                    .map_err(|_| "invalid block size exponent".to_string())?
            }
            "-o" | "--output" => output_dir = Some(PathBuf::from(take_value("-o")?)),
            "-k" | "--keys" => keys_path = Some(PathBuf::from(take_value("-k")?)),
            "-x" | "--skip-key-check" => skip_key_check = true,
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            other => {
                if other.starts_with('-') && other.len() > 1 {
                    return Err(format!("unknown flag: {other}"));
                }
                inputs.push(PathBuf::from(other));
            }
        }
        i += 1;
    }

    let mode = mode.ok_or("specify -C (compress) or -D (decompress)")?;
    if inputs.is_empty() {
        return Err("no input files given".into());
    }
    Ok(Args {
        mode,
        stream,
        fix_padding,
        ldm,
        level,
        block_exp,
        output_dir,
        keys_path,
        skip_key_check,
        inputs,
    })
}

fn print_usage() {
    eprintln!(
        "usage: nszrs (-C|-D) [-F] [-L] [-S|-B] [-l LEVEL] [-s EXP] [-o DIR] [-k KEYS] FILE...\n\
         \n\
         \x20 -C compress NSP->NSZ   -D decompress NSZ->NSP\n\
         \x20 -F fix padding         -L long-distance matching\n\
         \x20 -S solid (default)    -B block (parallel)\n\
         \x20 -l level (18)         -s block-size exponent (20)\n\
         \x20 -o output dir         -k prod.keys path (~/.switch/prod.keys)"
    );
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

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("nszrs: {e}");
            print_usage();
            std::process::exit(2);
        }
    };

    let keys = match Keys::load(
        args.keys_path.clone().unwrap_or_else(default_keys_path),
        !args.skip_key_check,
    ) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("nszrs: failed to load keys: {e}");
            std::process::exit(1);
        }
    };

    let mut failures = 0usize;
    for input in &args.inputs {
        let result = match args.mode {
            Mode::Decompress => {
                let out = out_path(input, args.output_dir.as_ref(), ".nsp");
                // Strict: a hash mismatch is an error and the output is removed.
                decompress_nsz(input, &out, &keys, args.fix_padding, true, true).map(|_| out)
            }
            Mode::Compress => {
                let out = out_path(input, args.output_dir.as_ref(), ".nsz");
                let compression = Compression {
                    level: args.level,
                    ldm: args.ldm,
                    block_size_exponent: match args.stream {
                        Stream::Solid => None,
                        Stream::Block => Some(args.block_exp),
                    },
                };
                compress_nsp(input, &out, &keys, &compression, args.fix_padding).map(|()| out)
            }
        };
        match result {
            Ok(out) => println!("nszrs: wrote {}", out.display()),
            Err(e) => {
                eprintln!("nszrs: {}: {e}", input.display());
                failures += 1;
            }
        }
    }
    if failures > 0 {
        std::process::exit(1);
    }
}

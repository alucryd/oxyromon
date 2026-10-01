//! `chdrs`: CHD compression and decompression, from the command line.

mod ui;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgMatches, Command, value_parser};

use chd_rs::{Chd, VerifyOutcome, sha1_hex};

fn cli() -> Command {
    Command::new("chdrs")
        .version(env!("CARGO_PKG_VERSION"))
        .about("CHD compression and decompression: images are compressed to CHD, CHDs extracted")
        .after_help(format!(
            "Commands: chdrs info <CHD> prints a CHD's contents, chdrs verify <CHD> checks its \
             checksums.\n\nCodecs: none, flac, huff, lzma, zlib and zstd for DVDs and hard disks; \
             none, cdfl, cdlz, cdzl and cdzs for CDs.\n\n{}",
            ui::AFTER_HELP
        ))
        .arg(input_arg("INPUTS", "Images to compress to CHD, CHDs to extract", true).num_args(1..))
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .help("Output directory [default: next to each input]")
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("hunk")
                .short('b')
                .long("hunk")
                .help("Hunk size in bytes, 16..=1048576 [default: 2048 for CDs, 4096 otherwise]")
                .value_parser(value_parser!(u32)),
        )
        .arg(
            Arg::new("sector-size")
                .long("sector-size")
                .help("Hard disk sector size in bytes: 512, 1024, 2048 or 4096 [default: 512]")
                .value_parser(value_parser!(u32)),
        )
        .arg(
            Arg::new("chs")
                .long("chs")
                .help("Hard disk geometry, cylinders,heads,sectors [default: guessed]")
                .value_parser(value_parser!(String)),
        )
        .arg(
            Arg::new("compression")
                .short('c')
                .long("compression")
                .help("Codecs to try per hunk, best wins")
                .value_delimiter(',')
                .default_value("zlib")
                .value_parser([
                    "none", "flac", "huff", "lzma", "zlib", "zstd", "cdfl", "cdlz", "cdzl", "cdzs",
                ]),
        )
        .arg(parent_arg())
}

fn info_command() -> Command {
    Command::new("info")
        .about("Print a CHD's contents")
        .after_help(ui::AFTER_HELP)
        .arg(input_arg("INPUT", "CHD to inspect", true))
        .arg(parent_arg())
}

fn verify_command() -> Command {
    Command::new("verify")
        .about("Verify a CHD against the checksums of its header")
        .after_help(ui::AFTER_HELP)
        .arg(input_arg("INPUT", "CHD to verify", true))
        .arg(parent_arg())
}

fn input_arg(id: &'static str, help: &'static str, required: bool) -> Arg {
    Arg::new(id)
        .help(help)
        .required(required)
        .value_parser(value_parser!(PathBuf))
}

fn parent_arg() -> Arg {
    Arg::new("parent")
        .short('p')
        .long("parent")
        .help("Parent CHD, when the CHD is a clone")
        .value_parser(value_parser!(PathBuf))
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    match args.get(1).and_then(|arg| arg.to_str()) {
        Some("info") => run_info(&args),
        Some("verify") => run_verify(&args),
        _ => ui::run(cli(), convert),
    }
}

/// Extract `input` if it is a CHD, compress it otherwise.
fn convert(
    input: &Path,
    matches: &ArgMatches,
    outputs: &mut ui::Outputs,
) -> Result<String, String> {
    let dir = ui::output_dir(input, matches)?;
    if ui::extension(input) == "chd" {
        let output = outputs.claim(ui::output_path(&dir, input, "iso"))?;
        let mut chd = open(input, matches).map_err(|error| error.to_string())?;
        let bar = ui::progress_bar(chd.info().logical_size, "Extracting", input);
        let result = chd.extract(&output, &mut |bytes| bar.inc(bytes));
        bar.finish_and_clear();
        result.map_err(|error| error.to_string())?;
        return Ok(ui::wrote(&output));
    }
    if matches!(ui::extension(input).as_str(), "cue" | "gdi") {
        return Err("CD creation is not implemented yet".to_string());
    }
    let compression = codecs(matches)?;
    let geometry = geometry(matches)?;
    let mut parent = match matches.get_one::<PathBuf>("parent") {
        Some(parent) => Some(Chd::open(parent).map_err(|error| error.to_string())?),
        None => None,
    };
    // like chdman createhd: the sector size comes from the option, then from
    // the parent, and 512 divides every image size 2048 does.
    let unit_bytes = matches
        .get_one::<u32>("sector-size")
        .copied()
        .unwrap_or_else(|| {
            parent
                .as_ref()
                .map_or(512, |parent| parent.info().unit_size)
        });
    let hunk_bytes = match matches.get_one::<u32>("hunk") {
        Some(&hunk) => {
            if !(16..=1024 * 1024).contains(&hunk) {
                return Err(format!("hunk size {hunk} is not in 16..=1048576"));
            }
            if let Some(parent) = &parent {
                let parent_hunk = parent.info().hunk_size;
                if hunk != parent_hunk {
                    return Err(format!(
                        "hunk size {hunk} does not match the parent CHD's {parent_hunk}"
                    ));
                }
            }
            hunk
        }
        None => parent
            .as_ref()
            .map_or(4096, |parent| parent.info().hunk_size),
    };
    let output = outputs.claim(ui::output_path(&dir, input, "chd"))?;
    let size = std::fs::metadata(input)
        .map_err(|error| error.to_string())?
        .len();
    let bar = ui::progress_bar(size, "Compressing", input);
    let result = chd_rs::create_hd(
        input,
        &output,
        unit_bytes,
        hunk_bytes,
        compression,
        parent.as_mut(),
        geometry,
        &mut |bytes| bar.inc(bytes),
    );
    bar.finish_and_clear();
    result.map_err(|error| error.to_string())?;
    let chd_size = std::fs::metadata(&output)
        .map_err(|error| error.to_string())?
        .len();
    let ratio = 100.0 * chd_size as f64 / size.max(1) as f64;
    Ok(format!("{} ({ratio:.1}%)", ui::wrote(&output)))
}

/// The `-c` codec names, laid out in the four compression slots of a header.
/// A short list leaves the trailing slots unused.
fn codecs(matches: &ArgMatches) -> Result<[u32; 4], String> {
    let names: Vec<&str> = matches
        .get_many::<String>("compression")
        .unwrap()
        .map(String::as_str)
        .collect();
    if names.len() > 4 {
        return Err("at most four codecs can be tried per hunk".to_string());
    }
    let mut slots = [0; 4];
    for (slot, name) in slots.iter_mut().zip(&names) {
        *slot = match *name {
            // The uncompressed slot is tagged 0, not by a fourcc.
            "none" => 0,
            name if name.starts_with("cd") => {
                return Err(format!(
                    "codec {name} is for CDs, and CD creation is not implemented yet"
                ));
            }
            name => u32::from_be_bytes(
                name.as_bytes()
                    .try_into()
                    .map_err(|_| format!("codec {name} is not supported"))?,
            ),
        };
    }
    Ok(slots)
}

/// The `--chs` cylinders, heads and sectors, when one was given.
fn geometry(matches: &ArgMatches) -> Result<Option<(u32, u32, u32)>, String> {
    let Some(value) = matches.get_one::<String>("chs") else {
        return Ok(None);
    };
    let Ok(numbers) = value
        .split(',')
        .map(str::parse::<u32>)
        .collect::<Result<Vec<u32>, _>>()
    else {
        return Err(format!("geometry {value} is not cylinders,heads,sectors"));
    };
    match numbers.as_slice() {
        &[cylinders, heads, sectors] => Ok(Some((cylinders, heads, sectors))),
        _ => Err(format!("geometry {value} is not cylinders,heads,sectors")),
    }
}

fn run_info(args: &[OsString]) -> ExitCode {
    let matches = parse(&info_command(), args);
    let input = matches.get_one::<PathBuf>("INPUT").unwrap();
    let chd = match open(input, &matches) {
        Ok(chd) => chd,
        Err(error) => return failed(input, &error),
    };
    print_info(input, &chd);
    ExitCode::SUCCESS
}

fn run_verify(args: &[OsString]) -> ExitCode {
    let matches = parse(&verify_command(), args);
    let input = matches.get_one::<PathBuf>("INPUT").unwrap();
    let mut chd = match open(input, &matches) {
        Ok(chd) => chd,
        Err(error) => return failed(input, &error),
    };
    let version = chd.info().version;
    match chd.verify_with(&mut |_| {}) {
        Err(error) => failed(input, &error),
        // `chdman` reports these and exits successfully.
        Ok(VerifyOutcome::Uncompressed) => {
            eprintln!("No verification to be done; CHD is uncompressed");
            ExitCode::SUCCESS
        }
        Ok(VerifyOutcome::NoChecksum) => {
            eprintln!("No verification to be done; CHD has no checksum");
            ExitCode::SUCCESS
        }
        Ok(VerifyOutcome::RawMismatch { expected, actual }) => {
            eprintln!("Error: Raw SHA1 in header = {}", sha1_hex(&expected));
            eprintln!("              actual SHA1 = {}", sha1_hex(&actual));
            ExitCode::FAILURE
        }
        Ok(VerifyOutcome::OverallMismatch { expected, actual }) => {
            let expected = expected.unwrap_or_default();
            eprintln!("Error: Overall SHA1 in header = {}", sha1_hex(&expected));
            eprintln!("                  actual SHA1 = {}", sha1_hex(&actual));
            ExitCode::FAILURE
        }
        Ok(VerifyOutcome::Ok) => {
            println!("Raw SHA1 verification successful!");
            if version >= 4 {
                println!("Overall SHA1 verification successful!");
            }
            ExitCode::SUCCESS
        }
    }
}

fn parse(command: &Command, args: &[OsString]) -> ArgMatches {
    match command.clone().try_get_matches_from(&args[1..]) {
        Ok(matches) => matches,
        Err(error) => error.exit(),
    }
}

fn open(input: &Path, matches: &ArgMatches) -> chd_rs::Result<Chd> {
    match matches.get_one::<PathBuf>("parent") {
        Some(parent) => Chd::open_with_parent(input, parent),
        None => Chd::open(input),
    }
}

fn failed(input: &Path, error: &chd_rs::Error) -> ExitCode {
    eprintln!("\u{2716} {}: {error}", input.display());
    ExitCode::FAILURE
}

/// Print a CHD's contents, line for line like `chdman info` does.
fn print_info(input: &Path, chd: &Chd) {
    let info = chd.info();
    let size = std::fs::metadata(input)
        .map(|metadata| metadata.len())
        .unwrap_or_default();
    println!("Input file:   {}", input.display());
    println!("File Version: {}", info.version);
    println!("Logical size: {} bytes", big_int_string(info.logical_size));
    println!(
        "Hunk Size:    {} bytes",
        big_int_string(u64::from(info.hunk_size))
    );
    println!("Total Hunks:  {}", big_int_string(info.hunk_count));
    println!(
        "Unit Size:    {} bytes",
        big_int_string(u64::from(info.unit_size))
    );
    println!("Total Units:  {}", big_int_string(info.unit_count));
    println!("Compression:  {}", info.compression_name);
    println!("CHD size:     {} bytes", big_int_string(size));
    if info.compression[0] != 0 && info.logical_size > 0 {
        let ratio = 100.0 * size as f64 / info.logical_size as f64;
        println!("Ratio:        {ratio:.1}%");
    }
    if let Some(sha1) = info.sha1 {
        println!("SHA1:         {}", sha1_hex(&sha1));
        if info.version >= 4 {
            println!(
                "Data SHA1:    {}",
                sha1_hex(&info.data_sha1.unwrap_or_default())
            );
        }
    }
    if let Some(parent) = info.parent_sha1 {
        println!("Parent SHA1:  {}", sha1_hex(&parent));
    }
    for entry in chd.metadata() {
        let tag = entry.tag.to_be_bytes();
        if tag
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
        {
            let name: String = tag.iter().map(|byte| *byte as char).collect();
            println!(
                "Metadata:     Tag='{name}'  Index={}  Length={} bytes",
                entry.index,
                entry.data.len()
            );
        } else {
            println!(
                "Metadata:     Tag={:08x}  Index={}  Length={} bytes",
                entry.tag,
                entry.index,
                entry.data.len()
            );
        }
        // the payload, printable characters only, to 60 characters
        print!("              ");
        for byte in entry.data.iter().take(60) {
            print!(
                "{}",
                if (0x20..=0x7e).contains(byte) {
                    *byte as char
                } else {
                    '.'
                }
            );
        }
        println!();
    }
}

/// Group digits in thousands, like `chdman`'s `big_int_string`.
fn big_int_string(value: u64) -> String {
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, byte) in digits.bytes().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(byte as char);
    }
    grouped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_is_well_formed() {
        cli().debug_assert();
        info_command().debug_assert();
        verify_command().debug_assert();
    }

    #[test]
    fn takes_several_inputs_and_defaults_to_zlib() {
        let matches = cli()
            .try_get_matches_from(["chdrs", "a.iso", "b.chd"])
            .unwrap();
        assert_eq!(matches.get_many::<PathBuf>("INPUTS").unwrap().count(), 2);
        assert_eq!(matches.get_one::<String>("compression").unwrap(), "zlib");
        assert!(matches.get_one::<PathBuf>("output").is_none());
        assert!(matches.get_one::<PathBuf>("parent").is_none());
    }

    #[test]
    fn takes_a_hunk_size_codecs_and_parent() {
        let matches = cli()
            .try_get_matches_from([
                "chdrs",
                "-b",
                "4096",
                "-c",
                "lzma,zstd",
                "-p",
                "parent.chd",
                "game.bin",
            ])
            .unwrap();
        assert_eq!(*matches.get_one::<u32>("hunk").unwrap(), 4096);
        assert_eq!(
            matches
                .get_many::<String>("compression")
                .unwrap()
                .collect::<Vec<_>>(),
            ["lzma", "zstd"]
        );
        assert_eq!(
            matches.get_one::<PathBuf>("parent").unwrap(),
            Path::new("parent.chd")
        );
    }

    #[test]
    fn takes_a_sector_size_and_a_geometry() {
        let matches = cli()
            .try_get_matches_from([
                "chdrs",
                "--sector-size",
                "1024",
                "--chs",
                "720,16,63",
                "game.bin",
            ])
            .unwrap();
        assert_eq!(*matches.get_one::<u32>("sector-size").unwrap(), 1024);
        assert_eq!(geometry(&matches).unwrap(), Some((720, 16, 63)));
    }

    #[test]
    fn a_malformed_geometry_is_rejected() {
        for value in ["720,16", "720,16,63,256", "720,x,63"] {
            let matches = cli()
                .try_get_matches_from(["chdrs", "--chs", value, "game.bin"])
                .unwrap();
            assert_eq!(
                geometry(&matches).unwrap_err(),
                format!("geometry {value} is not cylinders,heads,sectors")
            );
        }
    }

    #[test]
    fn info_and_verify_take_a_file_and_a_parent() {
        for command in [info_command(), verify_command()] {
            let matches = command
                .clone()
                .try_get_matches_from([command.get_name(), "-p", "parent.chd", "game.chd"])
                .unwrap();
            assert_eq!(
                matches.get_one::<PathBuf>("INPUT").unwrap(),
                Path::new("game.chd")
            );
            assert_eq!(
                matches.get_one::<PathBuf>("parent").unwrap(),
                Path::new("parent.chd")
            );
        }
    }

    #[test]
    fn groups_digits_in_thousands() {
        assert_eq!(big_int_string(0), "0");
        assert_eq!(big_int_string(999), "999");
        assert_eq!(big_int_string(1000), "1,000");
        assert_eq!(big_int_string(358400), "358,400");
        assert_eq!(big_int_string(29385792), "29,385,792");
    }
}

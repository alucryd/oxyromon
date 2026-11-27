//! `ps3rs`: PlayStation 3 JB folders to Redump ISOs and back, from the
//! command line.

mod ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgMatches, Command, value_parser};

fn cli() -> Command {
    Command::new("ps3rs")
        .version(env!("CARGO_PKG_VERSION"))
        .about("PlayStation 3 discs: each JB folder is rebuilt into its Redump ISO, each ISO extracted to its JB folder")
        .after_help(format!(
            "A JB folder needs its IRD; an ISO needs its IRD or its disc key.\n\n{}",
            ui::AFTER_HELP
        ))
        .arg(
            Arg::new("INPUTS")
                .help("JB folders to rebuild, or ISOs to extract")
                .required(true)
                .num_args(1..)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("ird")
                .long("ird")
                .help("The disc's IRD")
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("key")
                .long("key")
                .help("The disc key, 32 hex digits, as in a .dkey")
                .conflicts_with("ird")
                .value_parser(parse_key),
        )
        .arg(
            Arg::new("update")
                .long("update")
                .help("The PS3UPDAT.PUP to use when a JB folder has none")
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .help("Output directory [default: next to each input]")
                .value_parser(value_parser!(PathBuf)),
        )
}

fn parse_key(value: &str) -> Result<[u8; 16], String> {
    let value = value.trim();
    // ASCII first, so that the slices below fall on characters
    if !value.is_ascii() || value.len() != 32 {
        return Err("expected 32 hex digits".into());
    }
    let mut key = [0u8; 16];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[2 * i..2 * i + 2], 16)
            .map_err(|_| "expected 32 hex digits".to_string())?;
    }
    Ok(key)
}

fn main() -> ExitCode {
    ui::run(cli(), convert)
}

fn ird(matches: &ArgMatches) -> Result<Option<ps3_rs::Ird>, String> {
    matches
        .get_one::<PathBuf>("ird")
        .map(|path| ps3_rs::Ird::read(path).map_err(|e| format!("{}: {e}", path.display())))
        .transpose()
}

fn convert(
    input: &Path,
    matches: &ArgMatches,
    outputs: &mut ui::Outputs,
) -> Result<String, String> {
    if input.is_dir() {
        let ird = ird(matches)?.ok_or("a JB folder needs --ird")?;
        let name = input
            .file_name()
            .ok_or("a JB folder needs a name")?
            .to_string_lossy();
        let dir = ui::output_dir(input, matches)?;
        let output = outputs.claim(dir.join(format!("{name}.iso")))?;
        let update = matches.get_one::<PathBuf>("update");
        let size: u64 = ird
            .files()
            .map_err(|e| e.to_string())?
            .iter()
            .map(|file| file.size())
            .sum();
        let bar = ui::progress_bar(size, "Rebuilding", input);
        let result = ps3_rs::rebuild(
            &ird,
            &mut |path| {
                let local = input.join(path);
                match update {
                    Some(update) if path == ps3_rs::UPDATE_PATH && !local.is_file() => {
                        Some(update.clone())
                    }
                    _ => Some(local),
                }
            },
            &output,
            &mut |n| bar.inc(n),
        );
        bar.finish_and_clear();
        result.map_err(|e| e.to_string())?;
        return Ok(ui::wrote(&output));
    }
    match ui::extension(input).as_str() {
        "iso" => {
            let key = match (ird(matches)?, matches.get_one::<[u8; 16]>("key")) {
                (Some(ird), _) => ird.disc_key(),
                (None, Some(key)) => *key,
                (None, None) => return Err("an ISO needs --ird or --key".into()),
            };
            let size = std::fs::metadata(input).map_err(|e| e.to_string())?.len();
            let dir = ui::output_dir(input, matches)?;
            let stem = input.file_stem().unwrap_or_default();
            let output = outputs.claim(dir.join(stem))?;
            let bar = ui::progress_bar(size, "Extracting", input);
            let result = ps3_rs::extract(input, &key, &output, &mut |n| bar.inc(n));
            bar.finish_and_clear();
            let files = result.map_err(|e| e.to_string())?;
            Ok(format!("{} ({} files)", ui::wrote(&output), files.len()))
        }
        extension => Err(format!(
            "expected a JB folder or an ISO, not a .{extension}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_is_well_formed() {
        cli().debug_assert();
    }

    #[test]
    fn keys_are_32_hex_digits() {
        assert_eq!(
            parse_key("73819CEFE085B2C8FE8A92E7F81E84BC").unwrap()[..2],
            [0x73, 0x81]
        );
        assert!(parse_key("73819C").is_err());
        assert!(parse_key("zz819CEFE085B2C8FE8A92E7F81E84BC").is_err());
        // 32 bytes, but 8 characters
        assert!(parse_key("𝕩𝕩𝕩𝕩𝕩𝕩𝕩𝕩").is_err());
    }

    #[test]
    fn an_ird_and_a_key_are_one_or_the_other() {
        let key = "73819CEFE085B2C8FE8A92E7F81E84BC";
        assert!(
            cli()
                .try_get_matches_from(["ps3rs", "--ird", "a.ird", "--key", key, "a.iso"])
                .is_err()
        );
    }
}

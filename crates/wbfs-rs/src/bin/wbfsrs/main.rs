//! `wbfsrs`: WBFS creation and extraction, from the command line.

mod ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgMatches, Command, value_parser};

fn cli() -> Command {
    Command::new("wbfsrs")
        .version(env!("CARGO_PKG_VERSION"))
        .about("WBFS creation and extraction: GameCube and Wii ISOs are written to WBFS, WBFSs extracted")
        .after_help(format!(
            "A WBFS keeps only what the disc points to, as wit writes one: it is no longer the original disc. Writing one from a Wii ISO needs the Wii common key among the keys; wbfsrs ships none.\n\n{}",
            ui::AFTER_HELP
        ))
        .arg(
            Arg::new("INPUTS")
                .help("ISOs to write to WBFS, WBFSs to extract")
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
            Arg::new("keys")
                .long("keys")
                .help("Keys file holding the Wii common key: text, a BootMii keys.bin, or an otp.bin")
                .value_parser(value_parser!(PathBuf)),
        )
}

fn main() -> ExitCode {
    ui::run(cli(), convert)
}

/// Extract `input` if it is a WBFS, write it to one otherwise.
fn convert(
    input: &Path,
    matches: &ArgMatches,
    outputs: &mut ui::Outputs,
) -> Result<String, String> {
    let size = std::fs::metadata(input).map_err(|e| e.to_string())?.len();
    let dir = ui::output_dir(input, matches)?;

    if wbfs_rs::is_wbfs(input).map_err(|e| e.to_string())? {
        let output = outputs.claim(ui::output_path(&dir, input, "iso"))?;
        let bar = ui::progress_bar(size, "Extracting", input);
        let result = wbfs_rs::extract(input, &output, &mut |n| bar.inc(n));
        bar.finish_and_clear();
        result.map_err(|e| e.to_string())?;
        return Ok(ui::wrote(&output));
    }

    let keys = match matches.get_one::<PathBuf>("keys") {
        Some(path) => wbfs_rs::load_keys(path).map_err(|e| format!("{}: {e}", path.display()))?,
        None => Vec::new(),
    };
    let output = outputs.claim(ui::output_path(&dir, input, "wbfs"))?;
    let bar = ui::progress_bar(size, "Writing", input);
    let result = wbfs_rs::create(input, &output, &keys, &mut |n| bar.inc(n));
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
            .try_get_matches_from(["wbfsrs", "a.iso", "b.wbfs"])
            .unwrap();
        assert_eq!(matches.get_many::<PathBuf>("INPUTS").unwrap().count(), 2);
    }
}

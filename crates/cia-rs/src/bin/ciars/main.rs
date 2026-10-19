//! `ciars`: Nintendo 3DS CIA unpacking, from the command line.

mod ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgMatches, Command, value_parser};

fn cli() -> Command {
    Command::new("ciars")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Nintendo 3DS CIA unpacking: certificates, ticket, title metadata and contents")
        .after_help(format!(
            "A CIA unpacks to a set of files, so each one gets a folder named after it.\nContents are copied out undecrypted, the way ctrtool's --plain mode leaves them.\n\n{}",
            ui::AFTER_HELP
        ))
        .arg(
            Arg::new("INPUTS")
                .help("CIAs to unpack")
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
}

fn main() -> ExitCode {
    ui::run(cli(), convert)
}

/// Unpack one CIA into `<output>/<CIA stem>/`.
fn convert(
    input: &Path,
    matches: &ArgMatches,
    outputs: &mut ui::Outputs,
) -> Result<String, String> {
    if ui::extension(input) != "cia" {
        return Err("not a CIA".into());
    }
    let size = std::fs::metadata(input).map_err(|e| e.to_string())?.len();
    let dir = ui::output_dir(input, matches)?;
    let set = outputs.claim(dir.join(input.file_stem().unwrap_or_default()))?;
    std::fs::create_dir_all(&set).map_err(|e| format!("{}: {e}", set.display()))?;

    let bar = ui::progress_bar(size, "Unpacking", input);
    let result = cia_rs::unpack(input, &set, &cia_rs::UnpackOptions::default(), &mut |n| {
        bar.inc(n)
    });
    bar.finish_and_clear();
    let written = result.map_err(|e| e.to_string())?;
    let contents = written
        .iter()
        .filter(|file| matches!(file.part, cia_rs::Part::Content(_)))
        .count();
    Ok(format!("{} ({} contents)", ui::wrote(&set), contents))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_is_well_formed() {
        cli().debug_assert();
    }

    #[test]
    fn takes_several_inputs_without_an_output_directory() {
        let matches = cli()
            .try_get_matches_from(["ciars", "a.cia", "b.cia"])
            .unwrap();
        assert_eq!(matches.get_many::<PathBuf>("INPUTS").unwrap().count(), 2);
        assert!(matches.get_one::<PathBuf>("output").is_none());
    }
}

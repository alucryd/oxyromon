//! `gdirs`: Dreamcast GD-ROM CUE/BIN to GDI conversion, from the command line.

mod ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgMatches, Command, value_parser};

fn cli() -> Command {
    Command::new("gdirs")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Dreamcast GD-ROM CUE/BIN (Redump) to GDI conversion")
        .after_help(format!(
            "A GDI is a set of files, so each one gets a folder named after its CUE.\n\n{}",
            ui::AFTER_HELP
        ))
        .arg(
            Arg::new("INPUTS")
                .help("CUEs of CUE/BIN sets to convert")
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

/// Convert one CUE/BIN set into `<output>/<CUE stem>/`.
fn convert(
    input: &Path,
    matches: &ArgMatches,
    outputs: &mut ui::Outputs,
) -> Result<String, String> {
    if ui::extension(input) != "cue" {
        return Err("not a CUE".into());
    }
    let size = gdi_rs::input_size(input).map_err(|e| e.to_string())?;
    let dir = ui::output_dir(input, matches)?;
    let set = outputs.claim(dir.join(input.file_stem().unwrap_or_default()))?;
    std::fs::create_dir_all(&set).map_err(|e| format!("{}: {e}", set.display()))?;

    let bar = ui::progress_bar(size, "Converting", input);
    let result = gdi_rs::convert(input, &set, &mut |n| bar.inc(n));
    bar.finish_and_clear();
    let gdi = result.map_err(|e| e.to_string())?;
    Ok(format!(
        "{} ({} tracks)",
        ui::wrote(&gdi.gdi),
        gdi.tracks.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_is_well_formed() {
        cli().debug_assert();
    }

    #[test]
    fn takes_several_cues_without_an_output_directory() {
        let matches = cli()
            .try_get_matches_from(["gdirs", "a.cue", "b.cue"])
            .unwrap();
        assert_eq!(matches.get_many::<PathBuf>("INPUTS").unwrap().count(), 2);
        assert!(matches.get_one::<PathBuf>("output").is_none());
    }
}

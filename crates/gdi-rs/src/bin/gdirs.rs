//! `gdirs`: gdidrop's CUE/BIN to GDI conversion, as a CLI.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Arg, ArgMatches, Command, value_parser};
use indicatif::{ProgressBar, ProgressStyle};

fn cli() -> Command {
    Command::new("gdirs")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Dreamcast GD-ROM CUE/BIN (Redump) to GDI conversion")
        .arg(
            Arg::new("CUE")
                .help("The CUE of a CUE/BIN set with one BIN per track")
                .required(true)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .help("Output directory for the GDI and its tracks")
                .required(true)
                .value_parser(value_parser!(PathBuf)),
        )
}

fn main() -> ExitCode {
    match run(&cli().get_matches()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("gdirs: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(matches: &ArgMatches) -> Result<(), String> {
    let cue = matches.get_one::<PathBuf>("CUE").unwrap();
    let output = matches.get_one::<PathBuf>("output").unwrap();
    let fail = |e: gdi_rs::Error| format!("{}: {e}", cue.display());

    let size = gdi_rs::input_size(cue).map_err(fail)?;
    std::fs::create_dir_all(output).map_err(|e| format!("{}: {e}", output.display()))?;
    // Hidden automatically when stderr isn't a terminal.
    let bar = ProgressBar::new(size).with_style(
        ProgressStyle::with_template(
            "{prefix:>13.bold} [{bar:30}] {percent:>3}% {binary_bytes_per_sec:>12} ETA {eta:>3} {wide_msg}",
        )
        .unwrap()
        .progress_chars("=> "),
    );
    bar.set_prefix("Converting");
    bar.set_message(
        cue.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
    );
    let result = gdi_rs::convert(cue, output, &mut |n| bar.inc(n));
    bar.finish_and_clear();

    let gdi = result.map_err(fail)?;
    println!("gdirs: wrote {}", gdi.gdi.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_is_well_formed() {
        cli().debug_assert();
    }

    #[test]
    fn needs_an_output_directory() {
        assert!(cli().try_get_matches_from(["gdirs", "game.cue"]).is_err());
        cli()
            .try_get_matches_from(["gdirs", "game.cue", "-o", "out"])
            .unwrap();
    }
}

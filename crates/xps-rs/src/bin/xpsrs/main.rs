//! `xpsrs`: IPS and BPS patch application, from the command line.

mod ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgMatches, Command, value_parser};

fn cli() -> Command {
    Command::new("xpsrs")
        .version(env!("CARGO_PKG_VERSION"))
        .about("IPS and BPS patch application: each patch is applied to the source")
        .after_help(format!(
            "Each output is named after its patch, with the source's extension.\n\n{}",
            ui::AFTER_HELP
        ))
        .arg(
            Arg::new("INPUTS")
                .help("IPS or BPS patches to apply")
                .required(true)
                .num_args(1..)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("source")
                .short('s')
                .long("source")
                .help("The file the patches apply to")
                .required(true)
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

/// The patch's stem with the source's extension, as Flips names it.
fn output_name(patch: &Path, source: &Path) -> String {
    let stem = patch.file_stem().unwrap_or_default().to_string_lossy();
    match source.extension() {
        Some(extension) => format!("{stem}.{}", extension.to_string_lossy()),
        None => stem.into_owned(),
    }
}

fn convert(
    patch: &Path,
    matches: &ArgMatches,
    outputs: &mut ui::Outputs,
) -> Result<String, String> {
    let size = std::fs::metadata(patch).map_err(|e| e.to_string())?.len();
    let source = matches.get_one::<PathBuf>("source").unwrap();
    xps_rs::identify(patch).map_err(|e| e.to_string())?;
    let dir = ui::output_dir(patch, matches)?;
    let output = dir.join(output_name(patch, source));
    if std::path::absolute(&output).ok() == std::path::absolute(source).ok() {
        return Err(format!(
            "would overwrite \"{}\", the source",
            output.display()
        ));
    }
    let output = outputs.claim(output)?;

    let bar = ui::progress_bar(size, "Patching", patch);
    let result = xps_rs::apply(source, patch, &output, &mut |n| bar.inc(n));
    bar.finish_and_clear();
    let warning = result.map_err(|e| e.to_string())?;
    Ok(match warning {
        Some(warning) => format!("{} ({warning})", ui::wrote(&output)),
        None => ui::wrote(&output),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_is_well_formed() {
        cli().debug_assert();
    }

    #[test]
    fn takes_several_patches_and_needs_a_source() {
        let matches = cli()
            .try_get_matches_from(["xpsrs", "-s", "game.sfc", "a.ips", "b.bps"])
            .unwrap();
        assert_eq!(matches.get_many::<PathBuf>("INPUTS").unwrap().count(), 2);
        assert!(cli().try_get_matches_from(["xpsrs", "a.ips"]).is_err());
    }

    #[test]
    fn outputs_take_the_patch_name_and_the_source_extension() {
        assert_eq!(
            output_name(Path::new("in/Hack v1.1.bps"), Path::new("Game.sfc")),
            "Hack v1.1.sfc"
        );
        assert_eq!(
            output_name(Path::new("Hack.ips"), Path::new("Game")),
            "Hack"
        );
    }
}

//! `xdeltars`: xdelta3 patch decoding, from the command line.

mod ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgMatches, Command, value_parser};

fn cli() -> Command {
    Command::new("xdeltars")
        .version(env!("CARGO_PKG_VERSION"))
        .about("xdelta3 patch decoding: each patch is applied to the source")
        .after_help(format!(
            "Each output is named as its patch records, or after the patch otherwise.\n\n{}",
            ui::AFTER_HELP
        ))
        .arg(
            Arg::new("INPUTS")
                .help("xdelta3 patches to apply")
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

/// The name a patch's output takes: the one it records, or the patch's stem
/// with the source's extension.
fn output_name(patch: &Path, source: &Path, recorded: Option<&str>) -> String {
    if let Some(name) = recorded.and_then(|name| Path::new(name).file_name()) {
        return name.to_string_lossy().into_owned();
    }
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
    let header = xdelta_rs::read_header(patch).map_err(|e| e.to_string())?;
    let dir = ui::output_dir(patch, matches)?;
    let output = dir.join(output_name(patch, source, header.target_name.as_deref()));
    if std::path::absolute(&output).ok() == std::path::absolute(source).ok() {
        return Err(format!(
            "would overwrite \"{}\", the source",
            output.display()
        ));
    }
    let output = outputs.claim(output)?;

    let bar = ui::progress_bar(size, "Patching", patch);
    let result = xdelta_rs::decode(Some(source), patch, &output, &mut |n| bar.inc(n));
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
    fn takes_several_patches_and_needs_a_source() {
        let matches = cli()
            .try_get_matches_from(["xdeltars", "-s", "game.rom", "a.xdelta", "b.xdelta"])
            .unwrap();
        assert_eq!(matches.get_many::<PathBuf>("INPUTS").unwrap().count(), 2);
        assert!(
            cli()
                .try_get_matches_from(["xdeltars", "a.xdelta"])
                .is_err()
        );
    }

    #[test]
    fn outputs_are_named_as_the_patch_records() {
        let (patch, source) = (Path::new("in/Hack.xdelta"), Path::new("Game.rom"));
        assert_eq!(
            output_name(patch, source, Some("Game (Hack).rom")),
            "Game (Hack).rom"
        );
        // Only the name: a recorded path cannot climb out of the output directory.
        assert_eq!(
            output_name(patch, source, Some("../../etc/passwd")),
            "passwd"
        );
        assert_eq!(output_name(patch, source, None), "Hack.rom");
    }
}

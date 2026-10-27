//! `wuxrs`: Wii U disc images from the command line: WUDs are compressed to
//! WUX, WUXs decompressed to WUD, and either converted to WUA on request.

mod ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};

fn cli() -> Command {
    Command::new("wuxrs")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Wii U disc images: WUDs are compressed to WUX, WUXs decompressed to WUD")
        .after_help(format!(
            "WUA is Cemu's archive; NUS is a folder WUP Installer GX2 installs. A WUA wuxrs wrote is lossless: it converts back to the disc it came from, byte for byte, or to NUS folders, one per title. Its update partition is kept in a store shared by every WUA, a hidden {} folder next to them unless --updates says otherwise. WUA and NUS need the disc key and the Wii U common key among the keys; wuxrs ships none.\n\n{}",
            wux_rs::STORE_DIR,
            ui::AFTER_HELP
        ))
        .arg(
            Arg::new("INPUTS")
                .help("WUDs to compress, WUXs to decompress, WUAs to revert")
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
            Arg::new("format")
                .short('f')
                .long("format")
                .help("What to convert to [default: WUX for WUDs, WUD for WUXs, WUX for WUAs, or NUS for WUAs without their disc]")
                .value_parser(["wud", "wux", "wua", "nus"]),
        )
        .arg(
            Arg::new("add")
                .long("add")
                .help("NUS folder of an update or DLC to add to the WUA, with a single input")
                .action(ArgAction::Append)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("updates")
                .long("updates")
                .help("Folder of the update partitions WUAs share [default: next to each WUA]")
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("keys")
                .long("keys")
                .help("Keys file for WUA and NUS, as Cemu's keys.txt or a Wii U's otp.bin [default: Cemu's keys.txt]")
                .value_parser(value_parser!(PathBuf)),
        )
}

fn main() -> ExitCode {
    ui::run(cli(), convert)
}

/// Convert `input` to the format asked for, or the other of WUD and WUX.
fn convert(
    input: &Path,
    matches: &ArgMatches,
    outputs: &mut ui::Outputs,
) -> Result<String, String> {
    let size = std::fs::metadata(input).map_err(|e| e.to_string())?.len();
    let from = match ui::extension(input).as_str() {
        "wua" => "wua",
        _ => match wux_rs::Image::open(input)
            .map_err(|e| e.to_string())?
            .header()
        {
            Some(_) => "wux",
            None => "wud",
        },
    };
    let lossless = from == "wua"
        && wux_rs::disc_info(input)
            .map_err(|e| e.to_string())?
            .is_some();
    let to = match matches.get_one::<String>("format").map(String::as_str) {
        Some(to) if to == from => return Err(format!("already a {}", to.to_uppercase())),
        Some("wud" | "wux") if from == "wua" && !lossless => {
            return Err("this WUA does not hold what rebuilds its disc".into());
        }
        Some(to) => to,
        None if from == "wua" && lossless => "wux",
        None if from == "wua" => "nus",
        None if from == "wux" => "wud",
        None => "wux",
    };
    let added: Vec<&Path> = matches
        .get_many::<PathBuf>("add")
        .into_iter()
        .flatten()
        .map(PathBuf::as_path)
        .collect();
    if !added.is_empty() {
        if to != "wua" {
            return Err("--add only applies to WUA".into());
        }
        if matches.get_many::<PathBuf>("INPUTS").unwrap().count() > 1 {
            return Err("--add takes a single input".into());
        }
    }
    let keys = if matches!(to, "wua" | "nus") || from == "wua" {
        let path = match matches.get_one::<PathBuf>("keys") {
            Some(path) => path.clone(),
            None => {
                wux_rs::cemu_keys_path().ok_or("no data directory to find Cemu's keys.txt in")?
            }
        };
        wux_rs::load_keys(&path).map_err(|e| format!("{}: {e}", path.display()))?
    } else {
        Vec::new()
    };

    let dir = ui::output_dir(input, matches)?;
    // A NUS title is a folder, named after the input without its extension.
    let output = match to {
        "nus" => dir.join(input.file_stem().unwrap_or_default()),
        _ => ui::output_path(&dir, input, to),
    };
    let output = outputs.claim(output)?;
    // The update partitions next to the WUA, written or read.
    let wua = if from == "wua" {
        input
    } else {
        output.as_path()
    };
    let store = match matches.get_one::<PathBuf>("updates") {
        Some(store) => store.clone(),
        None => wua
            .parent()
            .unwrap_or(Path::new("."))
            .join(wux_rs::STORE_DIR),
    };
    let verb = match to {
        "wux" => "Compressing",
        "wud" => "Decompressing",
        "nus" => "Extracting",
        _ => "Converting",
    };
    // What `to_wua` reports covers the added folders' files too.
    let mut size = size;
    for dir in &added {
        for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let metadata = entry
                .and_then(|e| e.metadata())
                .map_err(|e| e.to_string())?;
            if metadata.is_file() {
                size += metadata.len();
            }
        }
    }
    let bar = ui::progress_bar(size, verb, input);
    let mut progress = |n| bar.inc(n);
    let result = match to {
        "wux" if from == "wua" => {
            wux_rs::wua_to_wux(input, &output, Some(&store), &keys, &mut progress)
        }
        "wud" if from == "wua" => {
            wux_rs::wua_to_wud(input, &output, Some(&store), &keys, &mut progress)
        }
        "wux" => wux_rs::compress(input, &output, &mut progress),
        "wud" => wux_rs::decompress(input, &output, &mut progress),
        "nus" if from == "wua" => wux_rs::wua_to_nus(input, &output, &keys, &mut progress),
        "nus" => wux_rs::to_nus(input, &output, &keys, &mut progress),
        _ => wux_rs::to_wua(
            &[&[input], &added[..]].concat(),
            &output,
            &keys,
            Some(&store),
            &mut progress,
        ),
    };
    bar.finish_and_clear();
    let stats = result.map_err(|e| e.to_string())?;
    // How big the output is next to the input.
    Ok(format!(
        "{} ({:.1}%)",
        ui::wrote(&output),
        stats.ratio_percent()
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
    fn takes_several_inputs_and_infers_the_format() {
        let matches = cli()
            .try_get_matches_from(["wuxrs", "a.wud", "b.wux"])
            .unwrap();
        assert_eq!(matches.get_many::<PathBuf>("INPUTS").unwrap().count(), 2);
        assert!(matches.get_one::<String>("format").is_none());
        assert!(matches.get_one::<PathBuf>("keys").is_none());
    }
}

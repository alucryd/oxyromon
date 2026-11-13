//! `orers`: ORE compression and decompression, from the command line.

mod ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};
use ore_rs::{Codec, Options, Profile};

fn cli() -> Command {
    Command::new("orers")
        .version(env!("CARGO_PKG_VERSION"))
        .about("ORE compression and decompression: GameCube and Wii ISOs and CD cue sheets are compressed, OREs decompressed, stores checked and repaired")
        .after_help(format!(
            "Writing an ORE from a Wii ISO needs the Wii common key among the keys; orers ships none.\n\n\
             A CD is given as its cue sheet, its tracks a file each; an SBI of the cue sheet's name beside it\n\
             is kept, and written back beside the cue sheet with the tracks.\n\n\
             A thin ORE finds its store as the nearest .ore-store up from its folder: keep the store in the\n\
             directory above every game using it. An ORE written from another ORE needs -o.\n\n{}",
            ui::AFTER_HELP
        ))
        .arg(
            Arg::new("INPUTS")
                .help("ISOs and cue sheets to compress, OREs to decompress, stores to check and repair")
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
            Arg::new("profile")
                .short('p')
                .long("profile")
                .help("stream: 1 MiB groups, quick to read; archive: 4 MiB groups, smaller")
                .value_parser(["stream", "archive"])
                .default_value("stream"),
        )
        .arg(
            Arg::new("compression")
                .short('c')
                .long("compression")
                .help("zstd: the quicker to read; lzma2: the smaller")
                .value_parser(["zstd", "lzma2"])
                .default_value("zstd"),
        )
        .arg(
            Arg::new("store")
                .long("store")
                .help("Store to write ISOs' chunks to, as thin OREs, and to attach OREs to; made if missing")
                .value_parser(value_parser!(PathBuf))
                .conflicts_with("detach"),
        )
        .arg(
            Arg::new("detach")
                .long("detach")
                .help("Write OREs as standalone OREs, holding their chunks themselves")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("purge")
                .long("purge")
                .help("Once a store is checked, purge it of what no game uses")
                .action(ArgAction::SetTrue),
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

/// How far a store's segment has to be no game's before a purge copies
/// what is left of it into a new one.
const PURGE_THRESHOLD: f64 = 0.5;

/// Check and repair `input` if it is a store, and purge it with `--purge`; with an ORE, attach it to a store,
/// detach it, or decompress it; compress anything else.
fn convert(
    input: &Path,
    matches: &ArgMatches,
    outputs: &mut ui::Outputs,
) -> Result<String, String> {
    let options = options(matches);
    if input.is_dir() {
        return store(input, matches, &options);
    }
    let size = std::fs::metadata(input).map_err(|e| e.to_string())?.len();
    let dir = ui::output_dir(input, matches)?;
    let store = matches.get_one::<PathBuf>("store");
    if let Some(store) = store
        && !store.exists()
    {
        ore_rs::create_store(store, &ore_rs::StoreOptions::default())
            .map_err(|e| format!("{}: {e}", store.display()))?;
    }
    if ore_rs::is_ore(input).map_err(|e| e.to_string())? {
        if store.is_some() || matches.get_flag("detach") {
            let output = outputs.claim(ui::output_path(&dir, input, "ore"))?;
            let bar = ui::progress_bar(size, "Rewriting", input);
            let result = match store {
                Some(store) => ore_rs::attach(input, &output, store, &options, &mut |n| bar.inc(n)),
                None => ore_rs::detach(input, &output, &options, &mut |n| bar.inc(n)),
            };
            bar.finish_and_clear();
            result.map_err(|e| e.to_string())?;
            return Ok(ui::wrote(&output));
        }
        // A CD comes back as its cue sheet, its tracks beside it
        let extension = match ore_rs::Reader::inspect(input) {
            Ok(reader) if reader.is_cd() => "cue",
            _ => "iso",
        };
        let output = outputs.claim(ui::output_path(&dir, input, extension))?;
        let bar = ui::progress_bar(size, "Decompressing", input);
        let result = ore_rs::decompress(input, &output, &mut |n| bar.inc(n));
        bar.finish_and_clear();
        result.map_err(|e| e.to_string())?;
        return Ok(ui::wrote(&output));
    }

    let keys = match matches.get_one::<PathBuf>("keys") {
        Some(path) => ore_rs::load_keys(path).map_err(|e| format!("{}: {e}", path.display()))?,
        None => Vec::new(),
    };
    let output = outputs.claim(ui::output_path(&dir, input, "ore"))?;
    let bar = ui::progress_bar(size, "Compressing", input);
    let result = match store {
        Some(store) => {
            ore_rs::compress_to_store(input, &output, store, &options, &keys, &mut |n| bar.inc(n))
        }
        None => ore_rs::compress(input, &output, &options, &keys, &mut |n| bar.inc(n)),
    };
    bar.finish_and_clear();
    let stats = result.map_err(|e| e.to_string())?;
    // How big the output is next to the original.
    Ok(format!(
        "{} ({:.1}%)",
        ui::wrote(&output),
        stats.ratio_percent()
    ))
}

/// Check the store `input`, repairing what its parity covers, then purge
/// it if asked to and nothing is lost.
fn store(input: &Path, matches: &ArgMatches, options: &Options) -> Result<String, String> {
    let checked = ore_rs::check_store(input, true).map_err(|e| e.to_string())?;
    if checked.lost_groups > 0 {
        let games: Vec<String> = checked
            .lost_games
            .iter()
            .map(|game| game.display().to_string())
            .collect();
        return Err(format!(
            "{} groups past repair, used by: {}",
            checked.lost_groups,
            games.join(", ")
        ));
    }
    let mut message = format!(
        "checked {}: {} segments, {} of {} damaged blocks repaired",
        input.display(),
        checked.segments,
        checked.repaired,
        checked.damaged
    );
    if matches.get_flag("purge") {
        let purged = ore_rs::purge(input, options, PURGE_THRESHOLD).map_err(|e| e.to_string())?;
        message += &format!(
            "; purged: {} games, {} to {} bytes",
            purged.games, purged.before, purged.after
        );
    }
    Ok(message)
}

fn options(matches: &ArgMatches) -> Options {
    Options {
        profile: match matches.get_one::<String>("profile").unwrap().as_str() {
            "archive" => Profile::Archive,
            _ => Profile::Stream,
        },
        codec: match matches.get_one::<String>("compression").unwrap().as_str() {
            "lzma2" => Codec::Lzma2,
            _ => Codec::Zstd,
        },
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
    fn takes_several_inputs_and_defaults_to_stream() {
        let matches = cli()
            .try_get_matches_from(["orers", "a.iso", "b.ore"])
            .unwrap();
        assert_eq!(matches.get_many::<PathBuf>("INPUTS").unwrap().count(), 2);
        assert_eq!(matches.get_one::<String>("profile").unwrap(), "stream");
        assert_eq!(matches.get_one::<String>("compression").unwrap(), "zstd");
    }

    #[test]
    fn a_store_and_detaching_exclude_each_other() {
        assert!(
            cli()
                .try_get_matches_from(["orers", "--store", "s", "--detach", "a.ore"])
                .is_err()
        );
    }
}

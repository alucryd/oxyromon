// SPDX-License-Identifier: GPL-3.0-or-later
//! `orers`: ORE compression and decompression, from the command line.

mod ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};
use ore_rs::{Codec, Options, Profile, StoreBatch};

fn cli() -> Command {
    Command::new("orers")
        .version(env!("CARGO_PKG_VERSION"))
        .about("ORE compression and decompression: GameCube, Wii, PlayStation 2 and PSP ISOs, cartridge ROMs and CD and GD-ROM cue sheets are compressed, OREs decompressed, stores checked and repaired")
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
                .help("zstd: the quicker to read; lzma2: the smaller [default: lzma2 for cartridge ROMs, zstd otherwise]")
                .value_parser(["zstd", "lzma2"]),
        )
        .arg(
            Arg::new("platform")
                .long("platform")
                .help("Cartridge the inputs are ROMs of, taken whole, their headers not checked [default: recognised by their headers]")
                .value_parser(ore_rs::CARTRIDGE_PLATFORMS),
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
            Arg::new("parity")
                .long("parity")
                .help("Parity shards a store's segment gets against its 239 data shards, 0 for none: a store made with it, or one set to it, its segments sealed otherwise written again by --compact [default: 16 for a store made]")
                .value_parser(value_parser!(u8).range(0..=16)),
        )
        .arg(
            Arg::new("compact")
                .long("compact")
                .help("Once a store is checked, compact it, dropping what no game uses")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("recompress")
                .long("recompress")
                .help("With --compact, decode and pack every group again: the smallest store, for a read and a write of all of it")
                .requires("compact")
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
    // Every input given with --store goes to it as one write, committed
    // after the last, or before a store given as an input is checked
    let mut batch = None;
    ui::run(cli(), |input, matches, outputs| {
        let result = convert(input, matches, outputs, &mut batch);
        let last = matches.get_many::<PathBuf>("INPUTS").unwrap().next_back();
        match batch.take() {
            Some(done) if last.is_some_and(|last| last == input) => commit(done, input)?,
            pending => batch = pending,
        }
        result
    })
}

/// Commit the batch, its bar named after `input`.
fn commit(batch: StoreBatch, input: &Path) -> Result<(), String> {
    let bar = ui::progress_bar(batch.unsealed(), "Committing", input);
    let committed = batch.commit(&mut |n| bar.inc(n));
    bar.finish_and_clear();
    committed
        .map(|_| ())
        .map_err(|e| format!("committing the store: {e}"))
}

/// How far a store's segment has to be no game's before a compaction copies
/// what is left of it into a new one.
const COMPACT_THRESHOLD: f64 = 0.5;

/// Check and repair `input` if it is a store, and compact it with `--compact`; with an ORE, attach it to a store,
/// detach it, or decompress it; compress anything else.
fn convert(
    input: &Path,
    matches: &ArgMatches,
    outputs: &mut ui::Outputs,
    batch: &mut Option<StoreBatch>,
) -> Result<String, String> {
    let options = options(matches, input);
    if input.is_dir() {
        // Checking a store takes its lock, which an open batch holds
        if let Some(open) = batch.take() {
            commit(open, input)?;
        }
        return store(input, matches, &options);
    }
    let size = std::fs::metadata(input).map_err(|e| e.to_string())?.len();
    let dir = ui::output_dir(input, matches)?;
    let store = matches.get_one::<PathBuf>("store");
    // Before the batch opens: once open, it holds the store's lock until
    // committed after the last input, which setting the parity waits on
    if let Some(store) = store
        && batch.is_none()
    {
        set_up_store(store, matches)?;
    }
    if ore_rs::is_ore(input).map_err(|e| e.to_string())? {
        if store.is_some() || matches.get_flag("detach") {
            let output = outputs.claim(ui::output_path(&dir, input, "ore"))?;
            let bar = ui::progress_bar(size, "Rewriting", input);
            let result = match store {
                Some(store) => {
                    open(batch, store, &options)?
                        .attach(input, &output, &options, &mut |n| bar.inc(n))
                }
                None => ore_rs::detach(input, &output, &options, &mut |n| bar.inc(n)),
            };
            bar.finish_and_clear();
            result.map_err(|e| e.to_string())?;
            return Ok(ui::wrote(&output));
        }
        // A CD comes back as its cue sheet, its tracks beside it; a ROM
        // with the extension it had
        let reader = ore_rs::Reader::inspect(input).ok();
        let rom = reader.as_ref().and_then(|reader| reader.file_name());
        let extension = match (&reader, rom.and_then(|name| Path::new(name).extension())) {
            (Some(reader), _) if reader.is_cd() => "cue",
            (_, Some(extension)) => extension.to_str().unwrap_or("iso"),
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
            open(batch, store, &options)?
                .compress(input, &output, &options, &keys, &mut |n| bar.inc(n))
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

/// The batch writing to `store`, opened by the first input to need it.
fn open<'a>(
    batch: &'a mut Option<StoreBatch>,
    store: &Path,
    options: &Options,
) -> Result<&'a mut StoreBatch, String> {
    if batch.is_none() {
        *batch = Some(
            StoreBatch::open(store, options).map_err(|e| format!("{}: {e}", store.display()))?,
        );
    }
    Ok(batch.as_mut().unwrap())
}

/// Make the store `store` if missing, with the parity asked for, or set an
/// existing one's to it.
fn set_up_store(store: &Path, matches: &ArgMatches) -> Result<(), String> {
    let parity = matches.get_one::<u8>("parity").copied();
    let result = if !store.exists() {
        let mut options = ore_rs::StoreOptions::default();
        if let Some(parity) = parity {
            options.parity = parity;
        }
        ore_rs::create_store(store, &options)
    } else if let Some(parity) = parity {
        ore_rs::set_parity(store, parity).map(|_| ())
    } else {
        Ok(())
    };
    result.map_err(|e| format!("{}: {e}", store.display()))
}

/// Check the store `input`, repairing what its parity covers, then compact
/// it if asked to and nothing is lost.
fn store(input: &Path, matches: &ArgMatches, options: &Options) -> Result<String, String> {
    if let Some(&parity) = matches.get_one::<u8>("parity") {
        ore_rs::set_parity(input, parity).map_err(|e| format!("{}: {e}", input.display()))?;
    }
    let size = ore_rs::store_size(input).map_err(|e| e.to_string())?;
    let bar = ui::progress_bar(size, "Checking", input);
    let checked = ore_rs::check_store(input, true, &mut |n| bar.inc(n));
    bar.finish_and_clear();
    let checked = checked.map_err(|e| e.to_string())?;
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
    if matches.get_flag("compact") {
        let size = ore_rs::store_size(input).map_err(|e| e.to_string())?;
        let bar = ui::progress_bar(size, "Compacting", input);
        let compacted = ore_rs::compact(
            input,
            options,
            COMPACT_THRESHOLD,
            matches.get_flag("recompress"),
            &mut |n| bar.inc(n),
        );
        bar.finish_and_clear();
        let compacted = compacted.map_err(|e| e.to_string())?;
        message += &format!(
            "; compacted: {} games, {} to {} bytes",
            compacted.games, compacted.before, compacted.after
        );
    }
    Ok(message)
}

fn options(matches: &ArgMatches, input: &Path) -> Options {
    let platform = matches.get_one::<String>("platform").and_then(|name| {
        ore_rs::CARTRIDGE_PLATFORMS
            .into_iter()
            .find(|known| known == name)
    });
    Options {
        profile: match matches.get_one::<String>("profile").unwrap().as_str() {
            "archive" => Profile::Archive,
            _ => Profile::Stream,
        },
        codec: match matches.get_one::<String>("compression").map(String::as_str) {
            Some("lzma2") => Codec::Lzma2,
            Some(_) => Codec::Zstd,
            // An input it cannot read fails on its own, as it is converted
            None => ore_rs::default_codec(input, platform).unwrap_or(Codec::Zstd),
        },
        platform,
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
        // The codec is each input's own unless given
        assert!(matches.get_one::<String>("compression").is_none());
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

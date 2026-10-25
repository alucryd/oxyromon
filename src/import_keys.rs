//! Import the keys oxyromon needs but does not ship into its data directory,
//! so that a headless install needs no emulator alongside it. For now, the Wii
//! U's: disc keys and the common key, in the format of Cemu's `keys.txt`.

use super::config::*;
use super::progress::*;
use anyhow::{Context, Result};
use clap::{Arg, ArgMatches, Command, value_parser};
use indexmap::IndexSet;
use indicatif::ProgressBar;
use sqlx::sqlite::SqliteConnection;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The Wii U keys in the data directory, one hex key per line.
pub const WIIU_KEYS_FILE: &str = "wiiu_keys.txt";

pub fn subcommand() -> Command {
    Command::new("import-keys")
        .about("Import Wii U keys into oxyromon")
        .arg(
            Arg::new("KEYS")
                .help("Set the key files to import, in the format of Cemu's keys.txt")
                .required(true)
                .num_args(1..)
                .index(1)
                .value_parser(value_parser!(PathBuf)),
        )
}

pub async fn main(
    _connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> Result<()> {
    let paths: Vec<&PathBuf> = matches.get_many::<PathBuf>("KEYS").unwrap().collect();
    let destination = get_data_directory()?.join(WIIU_KEYS_FILE);
    import_keys(progress_bar, &paths, &destination)
}

/// Merge the keys in `paths` into `destination`, keeping each key once, in
/// the order first seen.
fn import_keys(progress_bar: &ProgressBar, paths: &[&PathBuf], destination: &Path) -> Result<()> {
    let mut keys: IndexSet<wux_rs::Key> = IndexSet::new();
    if destination.is_file() {
        keys.extend(
            wux_rs::load_keys(destination)
                .with_context(|| format!("Failed to read \"{}\"", destination.display()))?,
        );
    }
    let before = keys.len();
    for path in paths {
        print_action(progress_bar, &format!("Reading \"{}\"", path.display()));
        let found = wux_rs::load_keys(path)
            .with_context(|| format!("Failed to read \"{}\"", path.display()))?;
        if found.is_empty() {
            print_warning(
                progress_bar,
                &format!("No keys found in \"{}\"", path.display()),
            );
        }
        keys.extend(found);
    }

    let added = keys.len() - before;
    if added == 0 {
        print_skip(progress_bar, "No new keys");
        return Ok(());
    }
    write_keys(&keys, destination)
        .with_context(|| format!("Failed to write \"{}\"", destination.display()))?;
    print_success(
        progress_bar,
        &format!("Imported {} new keys, {} in all", added, keys.len()),
    );
    Ok(())
}

/// Write `keys` next to `destination`, readable by its owner only, then move
/// them into place, so a failed write leaves the keys as they were.
fn write_keys(keys: &IndexSet<wux_rs::Key>, destination: &Path) -> Result<()> {
    let part = destination.with_extension("txt.part");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&part)?;
    for key in keys {
        let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
        writeln!(file, "{hex}")?;
    }
    file.sync_all()?;
    std::fs::rename(&part, destination)?;
    Ok(())
}

#[cfg(test)]
mod test_import_keys;

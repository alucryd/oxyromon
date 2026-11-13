//! Import the keys oxyromon needs but does not ship into its data directory,
//! so that a headless install needs no emulator alongside it:
//!
//! - The Wii U's and the Wii's: disc keys and common keys, in the format of
//!   Cemu's `keys.txt`, the Wii U, vWii and Wii common keys from a Wii U's OTP
//!   dump, and the Wii common key from a Wii's OTP dump or a BootMii
//!   `keys.bin`. They all go to one file, as every key is tried.
//! - The Switch's, from `prod.keys` and `title.keys`, merged by name into a
//!   file of the same format.

use super::common::to_hex;
use super::config::*;
use super::progress::*;
use anyhow::{Context, Result, bail};
use clap::{Arg, ArgMatches, Command, value_parser};
use indexmap::{IndexMap, IndexSet};
use indicatif::ProgressBar;
use sqlx::sqlite::SqliteConnection;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The Wii U and Wii keys in the data directory, one hex key per line.
pub const WIIU_KEYS_FILE: &str = "wiiu_keys.txt";
/// The Switch keys in the data directory, `name = HEX` lines as in `prod.keys`.
pub const SWITCH_KEYS_FILE: &str = "switch_keys.txt";

pub fn subcommand() -> Command {
    Command::new("import-keys")
        .about("Import Wii, Wii U and Switch keys into oxyromon")
        .arg(
            Arg::new("KEYS")
                .help("Set the key files to import, as Cemu's keys.txt, a Wii's or a Wii U's otp.bin, BootMii's keys.bin, or the Switch's prod.keys or title.keys")
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
    import_keys(progress_bar, &paths, &get_data_directory()?)
}

/// Merge the keys in `paths` into the key files in `data_directory`: the Wii U
/// and Wii keys keeping each key once, in the order first seen, the Switch keys
/// by name, a newer value replacing an older one. Nothing is written unless
/// every file was read.
fn import_keys(
    progress_bar: &ProgressBar,
    paths: &[&PathBuf],
    data_directory: &Path,
) -> Result<()> {
    let destination = data_directory.join(WIIU_KEYS_FILE);
    let switch_destination = data_directory.join(SWITCH_KEYS_FILE);
    let mut keys: IndexSet<wux_rs::Key> = IndexSet::new();
    if destination.is_file() {
        keys.extend(
            wux_rs::load_keys(&destination)
                .with_context(|| format!("Failed to read \"{}\"", destination.display()))?,
        );
    }
    let mut switch_keys: IndexMap<String, String> = IndexMap::new();
    if switch_destination.is_file() {
        switch_keys.extend(read_switch_keys(&switch_destination)?);
    }
    let before = keys.len();
    let switch_before = switch_keys.clone();
    for path in paths {
        print_action(progress_bar, &format!("Reading \"{}\"", path.display()));
        let found = read_switch_keys(path)?;
        if !found.is_empty() {
            switch_keys.extend(found);
            continue;
        }
        // Both read text alike; from binary dumps, wux-rs takes the Wii U and
        // vWii common keys, rvz-rs the Wii's, from a BootMii keys.bin too
        let found: Vec<wux_rs::Key> = match (wux_rs::load_keys(path), rvz_rs::load_keys(path)) {
            (Err(_), Err(error @ rvz_rs::Error::Io(_))) => {
                return Err(error)
                    .with_context(|| format!("Failed to read \"{}\"", path.display()));
            }
            (Err(_), Err(_)) => bail!(
                "Failed to read \"{}\": expected Cemu's keys.txt, a Wii U's or a Wii's otp.bin, BootMii's keys.bin, or the Switch's prod.keys or title.keys",
                path.display()
            ),
            (wiiu, wii) => wiiu
                .unwrap_or_default()
                .into_iter()
                .chain(wii.unwrap_or_default())
                .collect(),
        };
        if found.is_empty() {
            print_warning(
                progress_bar,
                &format!(
                    "No keys found in \"{}\": expected Cemu's keys.txt, a Wii U's or a Wii's otp.bin, BootMii's keys.bin, or the Switch's prod.keys or title.keys",
                    path.display()
                ),
            );
        }
        keys.extend(found);
    }

    let switch_added = switch_keys
        .iter()
        .filter(|(name, value)| switch_before.get(*name) != Some(*value))
        .count();
    let switch_lines: Vec<String> = switch_keys
        .iter()
        .map(|(name, value)| format!("{} = {}", name, value))
        .collect();
    // A tampered or incomplete prod.keys would only fail the next compression:
    // check it as nsz-rs will read it. title.keys alone has nothing to check.
    if switch_added > 0 && switch_keys.keys().any(|name| !is_rights_id(name)) {
        nsz_rs::keys::Keys::from_str(&switch_lines.join("\n"), true)
            .context("Failed to import the Switch keys")?;
    }

    let added = keys.len() - before;
    if added == 0 && switch_added == 0 {
        print_skip(progress_bar, "No new keys");
        return Ok(());
    }
    if added > 0 {
        write_lines(keys.iter().map(|key| to_hex(key)), &destination)
            .with_context(|| format!("Failed to write \"{}\"", destination.display()))?;
        print_success(
            progress_bar,
            &format!(
                "Imported {} new Wii U and Wii keys, {} in all",
                added,
                keys.len()
            ),
        );
    }
    if switch_added > 0 {
        write_lines(switch_lines.into_iter(), &switch_destination)
            .with_context(|| format!("Failed to write \"{}\"", switch_destination.display()))?;
        print_success(
            progress_bar,
            &format!(
                "Imported {} new Switch keys, {} in all",
                switch_added,
                switch_keys.len()
            ),
        );
    }
    Ok(())
}

/// The `name = HEX` lines of a Switch keys file, as nsz-rs reads them; none
/// when `path` is not text, or holds keys of another kind.
fn read_switch_keys(path: &Path) -> Result<Vec<(String, String)>> {
    let bytes =
        std::fs::read(path).with_context(|| format!("Failed to read \"{}\"", path.display()))?;
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return Ok(Vec::new());
    };
    Ok(text
        .lines()
        .filter_map(|line| {
            let (name, value) = line.split_once('=')?;
            let (name, value) = (name.trim(), value.trim());
            (!name.is_empty()
                && !value.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && value.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| (name.to_string(), value.to_uppercase()))
        })
        .collect())
}

/// Whether `name` is a `title.keys` rights ID rather than a `prod.keys` name.
fn is_rights_id(name: &str) -> bool {
    name.len() == 32 && name.chars().all(|c| c.is_ascii_hexdigit())
}

/// Write `lines` next to `destination`, readable by its owner only, then move
/// them into place, so a failed write leaves the keys as they were.
fn write_lines(lines: impl Iterator<Item = String>, destination: &Path) -> Result<()> {
    let part = destination.with_extension("txt.part");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&part)?;
    for line in lines {
        writeln!(file, "{}", line)?;
    }
    file.sync_all()?;
    std::fs::rename(&part, destination)?;
    Ok(())
}

#[cfg(test)]
mod test_import_keys;
#[cfg(test)]
mod test_import_otp;
#[cfg(test)]
mod test_import_switch;
#[cfg(test)]
mod test_import_switch_tampered;

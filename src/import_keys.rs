//! Import the keys oxyromon needs but does not ship into its data directory,
//! so that a headless install needs no emulator alongside it:
//!
//! - The Wii U's and the Wii's: disc keys and common keys, in the format of
//!   Cemu's `keys.txt`, the Wii U, vWii and Wii common keys from a Wii U's OTP
//!   dump, and the Wii common key from a Wii's OTP dump or a BootMii
//!   `keys.bin`. They all go to one file, as every key is tried.
//! - The Switch's, from `prod.keys` and `title.keys`, merged by name into a
//!   file of the same format.
//! - The 3DS's, from Azahar's `aes_keys.txt`, merged by name into a file of
//!   the same format.
//! - The PS3's disc keys, from Redump's `.dkey` files, one hex key a line,
//!   as every key is tried against a disc: an ISO with no IRD needs one.

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
/// The 3DS keys in the data directory, `name=HEX` lines as in Azahar's
/// `aes_keys.txt`.
pub const N3DS_KEYS_FILE: &str = "3ds_keys.txt";
/// The PS3 disc keys in the data directory, one hex key per line.
pub const PS3_KEYS_FILE: &str = "ps3_keys.txt";

pub fn subcommand() -> Command {
    Command::new("import-keys")
        .about("Import Wii, Wii U, Switch, 3DS and PS3 keys into oxyromon")
        .arg(
            Arg::new("KEYS")
                .help("Set the key files to import, as Cemu's keys.txt, a Wii's or a Wii U's otp.bin, BootMii's keys.bin, or the Switch's prod.keys or title.keys, or Azahar's aes_keys.txt, or Redump's PS3 .dkey files")
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
/// and Wii keys keeping each key once, in the order first seen, the Switch and
/// 3DS keys by name, a newer value replacing an older one. Nothing is written
/// unless every file was read.
fn import_keys(
    progress_bar: &ProgressBar,
    paths: &[&PathBuf],
    data_directory: &Path,
) -> Result<()> {
    let destination = data_directory.join(WIIU_KEYS_FILE);
    let switch_destination = data_directory.join(SWITCH_KEYS_FILE);
    let n3ds_destination = data_directory.join(N3DS_KEYS_FILE);
    let mut keys: IndexSet<wux_rs::Key> = IndexSet::new();
    if destination.is_file() {
        keys.extend(
            wux_rs::load_keys(&destination)
                .with_context(|| format!("Failed to read \"{}\"", destination.display()))?,
        );
    }
    let mut switch_keys: IndexMap<String, String> = IndexMap::new();
    if switch_destination.is_file() {
        switch_keys.extend(read_named_keys(&switch_destination)?);
    }
    let mut n3ds_keys: IndexMap<String, String> = IndexMap::new();
    if n3ds_destination.is_file() {
        n3ds_keys.extend(read_named_keys(&n3ds_destination)?);
    }
    let before = keys.len();
    let switch_before = switch_keys.clone();
    let n3ds_before = n3ds_keys.clone();
    let ps3_destination = data_directory.join(PS3_KEYS_FILE);
    let mut ps3_keys: IndexSet<[u8; 16]> = IndexSet::new();
    if ps3_destination.is_file() {
        ps3_keys.extend(load_ps3_keys(&ps3_destination)?);
    }
    let ps3_before = ps3_keys.len();
    for path in paths {
        print_action(progress_bar, &format!("Reading \"{}\"", path.display()));
        // a .dkey is one bare hex key, as a line of Cemu's keys.txt: only its
        // extension tells it apart
        if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("dkey"))
        {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read \"{}\"", path.display()))?;
            match parse_key(text.trim()) {
                Some(key) => ps3_keys.insert(key),
                None => bail!(
                    "Failed to read \"{}\": a .dkey holds one 32-digit hex key",
                    path.display()
                ),
            };
            continue;
        }
        let found = read_named_keys(path)?;
        // Azahar's names, which no prod.keys has
        if found.iter().any(|(name, _)| is_3ds_name(name)) {
            if let Some((name, _)) = found
                .iter()
                .find(|(name, value)| is_3ds_aes_key(name) && value.len() != 32)
            {
                bail!(
                    "Failed to read \"{}\": {} is not a 16-byte key",
                    path.display(),
                    name
                );
            }
            n3ds_keys.extend(found);
            continue;
        }
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
                "Failed to read \"{}\": expected Cemu's keys.txt, a Wii U's or a Wii's otp.bin, BootMii's keys.bin, or the Switch's prod.keys or title.keys, or Azahar's aes_keys.txt",
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
                    "No keys found in \"{}\": expected Cemu's keys.txt, a Wii U's or a Wii's otp.bin, BootMii's keys.bin, or the Switch's prod.keys or title.keys, or Azahar's aes_keys.txt",
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

    let n3ds_added = n3ds_keys
        .iter()
        .filter(|(name, value)| n3ds_before.get(*name) != Some(*value))
        .count();

    let added = keys.len() - before;
    let ps3_added = ps3_keys.len() - ps3_before;
    if added == 0 && switch_added == 0 && n3ds_added == 0 && ps3_added == 0 {
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
    if n3ds_added > 0 {
        write_lines(
            n3ds_keys
                .iter()
                .map(|(name, value)| format!("{}={}", name, value)),
            &n3ds_destination,
        )
        .with_context(|| format!("Failed to write \"{}\"", n3ds_destination.display()))?;
        print_success(
            progress_bar,
            &format!(
                "Imported {} new 3DS keys, {} in all",
                n3ds_added,
                n3ds_keys.len()
            ),
        );
    }
    if ps3_added > 0 {
        write_lines(ps3_keys.iter().map(|key| to_hex(key)), &ps3_destination)
            .with_context(|| format!("Failed to write \"{}\"", ps3_destination.display()))?;
        print_success(
            progress_bar,
            &format!(
                "Imported {} new PS3 disc keys, {} in all",
                ps3_added,
                ps3_keys.len()
            ),
        );
    }
    Ok(())
}

/// A 16-byte key, as 32 hex digits.
fn parse_key(hex: &str) -> Option<[u8; 16]> {
    if hex.len() != 32 || !hex.is_ascii() {
        return None;
    }
    let mut key = [0u8; 16];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(key)
}

/// The PS3 disc keys `import-keys` keeps, one hex key a line.
pub fn load_ps3_keys(path: &Path) -> Result<Vec<[u8; 16]>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read \"{}\"", path.display()))?;
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            parse_key(line)
                .with_context(|| format!("Failed to read \"{}\": {line} is no key", path.display()))
        })
        .collect()
}

/// The `name = HEX` lines of a Switch or 3DS keys file, as nsz-rs and Azahar
/// read them; none when `path` is not text, or holds keys of another kind.
fn read_named_keys(path: &Path) -> Result<Vec<(String, String)>> {
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

/// Whether `name` is one of Azahar's `aes_keys.txt`: a key slot's
/// (`slot0x2CKeyX`), or the key scrambler's generator constant.
fn is_3ds_name(name: &str) -> bool {
    name == "generatorConstant"
        || name.strip_prefix("slot0x").is_some_and(|rest| {
            rest.len() == 6
                && rest[..2].chars().all(|c| c.is_ascii_hexdigit())
                && matches!(&rest[2..], "KeyX" | "KeyY" | "KeyN")
        })
}

/// Whether the 3DS key `name` is an AES key, 16 bytes, rather than an RSA
/// modulus or exponent or an NFC secret.
fn is_3ds_aes_key(name: &str) -> bool {
    is_3ds_name(name)
        || name
            .strip_prefix("common")
            .is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit())
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
mod tests;

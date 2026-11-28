use super::common::*;
use super::config::*;
use super::database::*;
use super::mimetype::WUA_EXTENSION;
use super::model::Romfile;
use super::ore::recount_store;
use super::progress::*;
use super::prompt::*;
use super::util::*;
use super::wux::{STORE_DIR, has_extension};
use anyhow::{Context, Result};
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use sqlx::sqlite::SqliteConnection;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn subcommand() -> Command {
    Command::new("purge-roms")
        .about("Purge trashed, missing, and orphan ROM files")
        .arg(
            Arg::new("MISSING")
                .short('m')
                .long("missing")
                .help("Delete missing ROM files from the database")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("ORPHAN")
                .short('o')
                .long("orphan")
                .help("Delete ROM files without an associated ROM from the database")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("TRASH")
                .short('t')
                .long("trash")
                .help("Physically delete ROM files from the trash directories")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("FOREIGN")
                .short('f')
                .long("foreign")
                .help("Physically delete ROM files unknown to the database")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("YES")
                .short('y')
                .long("yes")
                .help("Automatically say yes to prompts")
                .required(false)
                .action(ArgAction::SetTrue),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> Result<()> {
    let answer_yes = matches.get_flag("YES");
    let missing = matches.get_flag("MISSING");
    let trash = matches.get_flag("TRASH");
    let orphan = matches.get_flag("ORPHAN");
    let foreign = matches.get_flag("FOREIGN");
    if missing {
        purge_missing_romfiles(connection, progress_bar).await?;
    }
    if trash {
        let romfiles = find_romfiles_in_trash(connection).await;
        purge_romfiles(connection, progress_bar, answer_yes, "trashed", romfiles).await?;
    }
    if orphan {
        let romfiles = find_orphan_romfiles(connection).await;
        purge_romfiles(connection, progress_bar, answer_yes, "orphan", romfiles).await?;
    }
    if foreign {
        purge_foreign_romfiles(connection, progress_bar, answer_yes).await?;
    } else if missing || trash || orphan {
        purge_unused_update_partitions(connection, progress_bar, answer_yes).await?;
    }
    for system in find_systems(connection).await {
        compute_system_completion(connection, progress_bar, &system).await?;
    }
    Ok(())
}

async fn purge_missing_romfiles(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
) -> Result<()> {
    print_subheader(progress_bar, "Processing missing ROM files");

    let romfiles = find_romfiles(connection).await;
    let kept_directories = get_kept_directories(connection).await?;
    let mut stores = HashSet::new();
    let mut count = 0;

    for romfile in romfiles {
        let path = romfile.as_common(connection).await?.path;
        if !path.is_file() {
            if let Some(store) = store_of_ore(&path) {
                stores.insert(store);
            }
            delete_romfile_by_id(connection, romfile.id).await;
            if let Some(directory) = path.parent() {
                remove_empty_directories(directory, &kept_directories).await?;
            }
            count += 1;
        }
    }

    if count > 0 {
        print_success(
            progress_bar,
            &format!("Deleted {} missing ROM file(s) from the database", count),
        );
    }

    recount_stores(progress_bar, stores).await
}

async fn purge_romfiles(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    answer_yes: bool,
    label: &str,
    romfiles: Vec<Romfile>,
) -> Result<()> {
    print_subheader(progress_bar, &format!("Processing {} ROM files", label));

    let mut count = 0;

    if !romfiles.is_empty() {
        print_subheader(progress_bar, "Summary:");
        for romfile in &romfiles {
            print_info(progress_bar, &romfile.path);
        }

        if answer_yes || confirm(true)? {
            let kept_directories = get_kept_directories(connection).await?;
            let mut transaction = begin_transaction(connection).await;
            let mut stores = HashSet::new();

            for romfile in &romfiles {
                let common_romfile = romfile.as_common(&mut transaction).await?;
                if common_romfile.path.is_file() {
                    let path = common_romfile.path.clone();
                    if let Some(store) = store_of_ore(&path) {
                        stores.insert(store);
                    }
                    common_romfile.delete(progress_bar, false).await?;
                    delete_romfile_by_id(&mut transaction, romfile.id).await;
                    if let Some(directory) = path.parent() {
                        remove_empty_directories(directory, &kept_directories).await?;
                    }
                    count += 1;
                }
            }

            commit_transaction(transaction).await;

            if count > 0 {
                print_success(
                    progress_bar,
                    &format!("Deleted {} {} ROM file(s)", count, label),
                );
            }

            return recount_stores(progress_bar, stores).await;
        }
    }

    Ok(())
}

async fn purge_foreign_romfiles(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    answer_yes: bool,
) -> Result<()> {
    print_subheader(progress_bar, "Processing foreign ROM files");
    let rom_directory = get_rom_directory(connection).await;
    let mut files = Vec::new();
    let mut known = HashSet::new();
    for entry in WalkDir::new(&rom_directory)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.path().is_file() {
            let relative_path = entry
                .path()
                .strip_prefix(&rom_directory)
                .context("Failed to retrieve relative path")?;
            if find_romfile_by_path(connection, relative_path.as_os_str().to_str().unwrap())
                .await
                .is_some()
            {
                known.insert(entry.path().to_path_buf());
            }
            files.push(entry.path().to_path_buf());
        }
    }
    let mut emptied_directories: HashSet<PathBuf> = HashSet::new();
    let mut stores = HashSet::new();
    let mut count = 0;
    for path in files {
        // An ORE store's segments and index belong to the thin OREs beneath
        // it, which the database knows, never to it; legacy update partition
        // stores are swept separately below.
        if path.components().any(|component| {
            component.as_os_str() == ore_rs::STORE_DIR || component.as_os_str() == STORE_DIR
        }) {
            continue;
        }
        if !known.contains(&path) {
            let relative_path = path
                .strip_prefix(&rom_directory)
                .context("Failed to retrieve relative path")?;
            {
                print_action(
                    progress_bar,
                    &format!(
                        "Foreign file found: \"{}\"",
                        relative_path.as_os_str().to_str().unwrap()
                    ),
                );
                if answer_yes || confirm(true)? {
                    if let Some(store) = store_of_ore(&path) {
                        stores.insert(store);
                    }
                    remove_file(progress_bar, &path, false).await?;
                    if let Some(directory) = path.parent() {
                        emptied_directories.insert(directory.to_path_buf());
                    }
                    count += 1;
                }
            }
        }
    }
    // once the walk is over, so that no directory is deleted while being read
    let kept_directories = get_kept_directories(connection).await?;
    for directory in &emptied_directories {
        remove_empty_directories(directory, &kept_directories).await?;
    }
    if count > 0 {
        print_success(
            progress_bar,
            &format!(
                "Deleted {} foreign ROM file(s) from the ROM directory",
                count
            ),
        );
    }
    recount_stores(progress_bar, stores).await?;
    purge_unused_update_partitions(connection, progress_bar, answer_yes).await
}

/// Delete legacy `.update-partitions` entries that no WUA in the database
/// names any more, and remove the store directory once empty. Fails closed:
/// if any WUA in the database cannot be read, all entries are kept.
async fn purge_unused_update_partitions(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    answer_yes: bool,
) -> Result<()> {
    let rom_directory = get_rom_directory(connection).await;
    let store_dirs: Vec<PathBuf> = WalkDir::new(&rom_directory)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir() && e.file_name() == STORE_DIR)
        .map(|e| e.into_path())
        .collect();
    if store_dirs.is_empty() {
        return Ok(());
    }

    // Collect every entry named by any WUA in the database. Fail closed if
    // any WUA cannot be opened or parsed so its entries stay intact.
    let mut referenced: HashSet<String> = HashSet::new();
    for romfile in find_romfiles(connection).await {
        let path = rom_directory.join(&romfile.path);
        if !has_extension(&path, WUA_EXTENSION) {
            continue;
        }
        match wux_rs::store_entries(&path) {
            Ok(entries) => referenced.extend(entries),
            Err(error) => {
                print_warning(
                    progress_bar,
                    &format!(
                        "Failed to read \"{}\", keeping every update partition: {error}",
                        path.display()
                    ),
                );
                return Ok(());
            }
        }
    }

    let kept_directories = get_kept_directories(connection).await?;
    let mut count = 0;
    for store_dir in store_dirs {
        let mut candidates = Vec::new();
        for entry in WalkDir::new(&store_dir).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let needed = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| referenced.contains(name));
            if !needed {
                candidates.push(path.to_path_buf());
            }
        }
        for path in candidates {
            let relative_path = path
                .strip_prefix(&rom_directory)
                .context("Failed to retrieve relative path")?;
            print_action(
                progress_bar,
                &format!(
                    "Unused update partition found: \"{}\"",
                    relative_path.as_os_str().to_str().unwrap()
                ),
            );
            if answer_yes || confirm(true)? {
                remove_file(progress_bar, &path, false).await?;
                count += 1;
            }
        }
        remove_empty_directories(&store_dir, &kept_directories).await?;
    }
    if count > 0 {
        print_success(
            progress_bar,
            &format!("Deleted {} unused update partition(s)", count),
        );
    }
    Ok(())
}

/// The ORE store whose chunks a game at `path` was using, if it had one:
/// a thin ORE names its store in its header, but a deleted file cannot be
/// read, so the nearest store above its path stands in. Only OREs ask.
fn store_of_ore(path: &Path) -> Option<PathBuf> {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ore"))
        .then(|| ore_rs::find_store(path))
        .flatten()
}

/// Count the games of every store in `stores` again: a game purged never
/// gave its chunks back, and the sizes the store reports divide by the
/// references it still holds. One recount per store, however many of its
/// games left.
async fn recount_stores(progress_bar: &ProgressBar, stores: HashSet<PathBuf>) -> Result<()> {
    for store in stores {
        let name = store.display().to_string();
        print_action(progress_bar, &format!("Recounting \"{name}\""));
        recount_store(progress_bar, &store).await?;
        stop_action(progress_bar);
    }
    Ok(())
}

#[cfg(test)]
mod tests;

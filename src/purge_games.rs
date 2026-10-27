use super::common::*;
use super::database::*;
use super::model::*;
use super::progress::*;
use super::prompt::*;
use super::util::*;
use anyhow::{Context, Result};
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use sqlx::sqlite::SqliteConnection;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

pub fn subcommand() -> Command {
    Command::new("purge-games")
        .about("Move the files of games to the trash")
        .arg(
            Arg::new("SYSTEM")
                .short('s')
                .long("system")
                .help("Select systems by name")
                .required(false)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("GAME")
                .short('g')
                .long("game")
                .help("Select games by exact name")
                .required(false)
                .action(ArgAction::Append),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> Result<()> {
    let systems = match matches.get_many::<String>("SYSTEM") {
        Some(system_names) => resolve_systems_by_name_like(connection, system_names).await,
        None => prompt_for_systems(connection, None, false, false, false).await?,
    };
    let game_names: Option<Vec<&String>> =
        matches.get_many::<String>("GAME").map(Iterator::collect);
    for system in systems {
        print_header(
            progress_bar,
            &format!("Purging games of \"{}\"", system.name),
        );
        let mut games = find_games_by_system_id(connection, system.id).await;
        if let Some(game_names) = &game_names {
            for game_name in game_names {
                if !games.iter().any(|game| &game.name == *game_name) {
                    print_warning(progress_bar, &format!("Game \"{}\" not found", game_name));
                }
            }
        }
        // only games with files have anything to purge
        games.retain(|game| game.completion > 0);
        let games = match &game_names {
            Some(game_names) => {
                games.retain(|game| game_names.contains(&&game.name));
                games
            }
            None => prompt_for_games(games, false)?,
        };
        if !games.is_empty() {
            purge_games(connection, progress_bar, &games).await?;
        }
        print_separator(progress_bar);
    }
    Ok(())
}

/// Move the files of `games` to their system's Trash and unlink them from their
/// ROMs, so the games read as missing and `sort-roms` leaves the files there.
/// A file that also holds ROMs of a game not being purged is left in place.
pub async fn purge_games(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    games: &[Game],
) -> Result<()> {
    let game_ids: HashSet<i64> = games.iter().map(|game| game.id).collect();
    let kept_directories = get_kept_directories(connection).await?;
    // each system with its Trash and the playlists of its purged games
    let mut systems: BTreeMap<i64, (System, PathBuf, BTreeSet<i64>)> = BTreeMap::new();

    for game in games {
        print_subheader(progress_bar, &format!("Purging \"{}\"", game.name));
        let (_, trash_directory, playlist_ids) = match systems.entry(game.system_id) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let system = find_system_by_id(connection, game.system_id).await;
                let trash_directory = get_trash_directory(connection, Some(&system)).await?;
                entry.insert((system, trash_directory, BTreeSet::new()))
            }
        };

        let romfile_ids: BTreeSet<i64> = find_roms_with_romfile_by_game_id(connection, game.id)
            .await
            .into_iter()
            .filter_map(|rom| rom.romfile_id)
            .collect();
        for romfile_id in romfile_ids {
            let roms = find_roms_by_romfile_id(connection, romfile_id).await;
            if !is_purged(&roms, &game_ids) {
                print_skip(progress_bar, "File shared with another game, skipping");
                continue;
            }
            // a child CHD of another game needs this file as its parent
            let mut children_purged = true;
            for child in find_romfiles_by_parent_id(connection, romfile_id).await {
                let child_roms = find_roms_by_romfile_id(connection, child.id).await;
                children_purged &= is_purged(&child_roms, &game_ids);
            }
            if !children_purged {
                print_skip(
                    progress_bar,
                    "File is the parent of another game's, skipping",
                );
                continue;
            }
            let romfile = find_romfile_by_id(connection, romfile_id).await;
            if trash_romfile(
                connection,
                progress_bar,
                &romfile,
                game,
                trash_directory,
                &kept_directories,
            )
            .await?
            {
                for rom in roms {
                    update_rom_romfile(connection, rom.id, None).await;
                }
            }
        }
        if let Some(playlist_id) = game.playlist_id {
            playlist_ids.insert(playlist_id);
        }
    }

    for (system, trash_directory, playlist_ids) in systems.values() {
        let system_games = find_games_by_system_id(connection, system.id).await;
        // a playlist goes once none of the discs it lists has a file left,
        // whether purged in this run or an earlier one
        for playlist_id in playlist_ids {
            let playlist_games: Vec<&Game> = system_games
                .iter()
                .filter(|game| game.playlist_id == Some(*playlist_id))
                .collect();
            let Some(first_game) = playlist_games.first() else {
                continue;
            };
            let mut has_files = false;
            for game in &playlist_games {
                if !find_roms_with_romfile_by_game_id(connection, game.id)
                    .await
                    .is_empty()
                {
                    has_files = true;
                    break;
                }
            }
            if has_files {
                continue;
            }
            let playlist = find_romfile_by_id(connection, *playlist_id).await;
            if trash_romfile(
                connection,
                progress_bar,
                &playlist,
                first_game,
                trash_directory,
                &kept_directories,
            )
            .await?
            {
                for game in &playlist_games {
                    update_game_playlist(connection, game.id, None).await;
                }
            }
        }
        compute_system_completion(connection, progress_bar, system).await?;
    }

    Ok(())
}

fn is_purged(roms: &[Rom], game_ids: &HashSet<i64>) -> bool {
    roms.iter().all(|rom| game_ids.contains(&rom.game_id))
}

/// Move a file to the Trash, keeping the game's folder when the file sits in
/// one (arcade, JB folder and NUS titles), so same-named files do not collide.
/// Returns false when the move was refused, true once the file is gone from its
/// place, whether moved or already missing.
async fn trash_romfile(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    romfile: &Romfile,
    game: &Game,
    trash_directory: &Path,
    kept_directories: &HashSet<PathBuf>,
) -> Result<bool> {
    let common_romfile = romfile.as_common(connection).await?;
    let path = common_romfile.path.clone();
    if !path.exists() {
        delete_romfile_by_id(connection, romfile.id).await;
        return Ok(true);
    }
    let components: Vec<_> = path.components().collect();
    let new_path = match components
        .iter()
        .rposition(|component| component.as_os_str() == game.name.as_str())
    {
        Some(index) => components[index..]
            .iter()
            .fold(trash_directory.to_path_buf(), |path, component| {
                path.join(component)
            }),
        None => trash_directory.join(path.file_name().context("ROM file has no name")?),
    };
    if new_path.exists() {
        print_warning(
            progress_bar,
            &format!("\"{}\" already exists, skipping", new_path.display()),
        );
        return Ok(false);
    }
    common_romfile
        .rename(progress_bar, &new_path, false)
        .await?
        .update(connection, progress_bar, romfile.id)
        .await?;
    if let Some(directory) = path.parent() {
        remove_empty_directories(directory, kept_directories).await?;
    }
    Ok(true)
}

#[cfg(test)]
mod test_purge_games;
#[cfg(test)]
mod test_purge_games_clash;
#[cfg(test)]
mod test_purge_games_folder;
#[cfg(test)]
mod test_purge_games_missing;
#[cfg(test)]
mod test_purge_games_parent;
#[cfg(test)]
mod test_purge_games_playlist;
#[cfg(test)]
mod test_purge_games_shared;

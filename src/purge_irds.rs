use super::common::*;
use super::database::*;
use super::model::*;
use super::progress::*;
use super::prompt::*;
use super::util::*;
use anyhow::Result;
use clap::{Arg, ArgMatches, Command};
use indicatif::ProgressBar;
use sqlx::sqlite::SqliteConnection;
use std::collections::HashSet;

pub fn subcommand() -> Command {
    Command::new("purge-irds")
        .about("Unassociate games from IRD files")
        .arg(
            Arg::new("GAMES")
                .help("Set the game names to purge")
                .required(false)
                .num_args(1..)
                .index(1),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> Result<()> {
    let system = prompt_for_system_like(connection, None, "%PlayStation 3%").await?;
    let mut games = find_games_by_system_id(connection, system.id).await;

    // Filter to only jbfolder games
    games.retain(|game| game.jbfolder);

    if games.is_empty() {
        print_info(progress_bar, "No IRD games found");
        return Ok(());
    }

    let game_names: Option<Vec<&String>> = matches.get_many::<String>("GAMES").map(|g| g.collect());

    if let Some(game_names) = game_names {
        for game_name in game_names {
            if let Some(game) = games.iter().find(|game| &game.name == game_name) {
                purge_ird(connection, progress_bar, game).await?;
            } else {
                print_warning(progress_bar, &format!("Game \"{}\" not found", game_name));
            }
        }
    } else {
        // Interactive mode
        while let Some(game) = prompt_for_game(&games, None)? {
            let game_id = game.id;
            purge_ird(connection, progress_bar, game).await?;
            games.retain(|g| g.id != game_id);
            if games.is_empty() {
                print_info(progress_bar, "No more IRD games to purge");
                break;
            }
        }
    }

    compute_system_completion(connection, progress_bar, &system).await?;

    Ok(())
}

pub async fn purge_ird(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    game: &Game,
) -> Result<()> {
    print_header(progress_bar, &format!("Purging IRD for \"{}\"", game.name));

    let mut transaction = begin_transaction(connection).await;
    let system = find_system_by_id(&mut transaction, game.system_id).await;
    let system_directory = get_system_directory(&mut transaction, &system).await?;
    let trash_directory = system_directory.join("Trash");

    // Find all parent roms for this game (should typically be one)
    let parent_roms = find_roms_by_game_id_no_parents(&mut transaction, game.id).await;

    // Collect all romfile IDs to trash
    let mut romfile_ids: HashSet<i64> = HashSet::new();

    for parent_rom in parent_roms {
        // Find all child roms
        let child_roms = find_roms_by_parent_id(&mut transaction, parent_rom.id).await;

        print_action(
            progress_bar,
            &format!(
                "Deleting {} child roms for parent \"{}\"",
                child_roms.len(),
                parent_rom.name
            ),
        );

        // Collect romfile IDs from child roms
        for child_rom in &child_roms {
            if let Some(romfile_id) = child_rom.romfile_id {
                romfile_ids.insert(romfile_id);
            }
        }

        // Delete child roms
        for child_rom in child_roms {
            delete_rom_by_id(&mut transaction, child_rom.id).await;
        }
    }

    // Move romfiles to trash
    if !romfile_ids.is_empty() {
        print_action(
            progress_bar,
            &format!("Moving {} romfiles to trash", romfile_ids.len()),
        );
        for romfile_id in romfile_ids {
            let romfile = find_romfile_by_id(&mut transaction, romfile_id).await;
            let common_romfile = romfile.as_common(&mut transaction).await?;
            if common_romfile.path.exists() {
                common_romfile
                    .rename(
                        progress_bar,
                        &trash_directory.join(common_romfile.path.file_name().unwrap()),
                        false,
                    )
                    .await?
                    .update(&mut transaction, progress_bar, romfile_id)
                    .await?;
            }
        }
    }

    // Mark game as not jbfolder
    update_game_jbfolder(&mut transaction, game.id, false).await;

    commit_transaction(transaction).await;

    print_success(progress_bar, "IRD purged successfully");

    Ok(())
}

#[cfg(test)]
mod test_ird;

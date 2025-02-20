use super::chdman;
use super::chdman::AsChd;
use super::common::*;
use super::config::*;
use super::database::*;
use super::dolphin;
use super::dolphin::AsRvz;
use super::maxcso;
use super::maxcso::AsXso;
use super::mimetype::*;
use super::model::*;
use super::nsz;
use super::nsz::AsNsz;
use super::prompt::*;
use super::sevenzip;
use super::sevenzip::AsArchive;
use super::util::*;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use simple_error::SimpleResult;
use sqlx::sqlite::SqliteConnection;
use std::collections::HashMap;

pub fn subcommand() -> Command {
    Command::new("check-roms")
        .about("Check ROM files' integrity")
        .arg(
            Arg::new("ALL")
                .short('a')
                .long("all")
                .help("Check all systems")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("GAME")
                .short('g')
                .long("game")
                .help("Select games by name")
                .required(false)
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("SIZE")
                .short('s')
                .long("size")
                .help("Recalculate ROM file sizes")
                .required(false)
                .action(ArgAction::SetTrue),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let systems = prompt_for_systems(connection, None, false, false, matches.get_flag("ALL")).await?;
    for system in systems {
        progress_bar.println(format!("Processing \"{}\"", system.name));
        let games = match matches.get_many::<String>("GAME") {
            Some(game_names) => {
                let mut games: Vec<Game> = vec![];
                for game_name in game_names {
                    games.append(
                        &mut find_full_games_by_name_and_system_id(
                            connection, game_name, system.id,
                        )
                        .await,
                    );
                }
                games.dedup_by_key(|game| game.id);
                prompt_for_games(games, cfg!(test))?
            }
            None => find_full_games_by_system_id(connection, system.id).await,
        };

        if games.is_empty() {
            if matches.index_of("GAME").is_some() {
                progress_bar.println("No matching game");
            }
            continue;
        }
        check_system(
            connection,
            progress_bar,
            &system,
            games,
            matches.get_flag("SIZE"),
        )
        .await?;
        progress_bar.println("");
    }
    Ok(())
}

async fn check_system(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    games: Vec<Game>,
    size: bool,
) -> SimpleResult<()> {
    let roms = find_original_roms_with_romfile_by_game_ids(
        connection,
        &games.iter().map(|game| game.id).collect::<Vec<i64>>(),
    )
    .await;
    let romfiles = find_romfiles_by_ids(
        connection,
        roms.iter()
            .map(|rom| rom.romfile_id.unwrap())
            .collect::<Vec<i64>>()
            .as_slice(),
    )
    .await;
    let mut roms_by_romfile_id: HashMap<i64, Vec<&Rom>> = HashMap::new();
    roms.iter().for_each(|rom| {
        let group = roms_by_romfile_id
            .entry(rom.romfile_id.unwrap())
            .or_default();
        group.push(rom);
    });
    let header = find_header_by_system_id(connection, system.id).await;

    let mut transaction = begin_transaction(connection).await;

    let mut errors = 0;

    for romfile in &romfiles {
        let romfile_path = romfile.as_common(&mut transaction).await?.path;
        let romfile_extension = romfile_path.extension().unwrap().to_str().unwrap();
        let romfile_roms = roms_by_romfile_id.remove(&romfile.id).unwrap();

        progress_bar.println(format!(
            "Processing \"{}\"",
            romfile_path.file_name().unwrap().to_str().unwrap()
        ));

        let result;
        if ARCHIVE_EXTENSIONS.contains(&romfile_extension) {
            if sevenzip::get_version().await.is_err() {
                progress_bar.println("Please install sevenzip");
                break;
            }
            result = check_archive(
                &mut transaction,
                progress_bar,
                &header,
                romfile,
                romfile_roms,
            )
            .await;
        } else if CHD_EXTENSION == romfile_extension {
            if chdman::get_version().await.is_err() {
                progress_bar.println("Please install chdman");
                break;
            }
            let chd_romfile = match romfile.parent_id {
                Some(parent_id) => {
                    let parent_chd_romfile = find_romfile_by_id(&mut transaction, parent_id).await;
                    romfile
                        .as_common(&mut transaction)
                        .await?
                        .as_chd_with_parent(
                            parent_chd_romfile
                                .as_common(&mut transaction)
                                .await?
                                .as_chd()
                                .await?,
                        )
                        .await?
                }
                None => romfile.as_common(&mut transaction).await?.as_chd().await?,
            };
            result = chd_romfile
                .check(&mut transaction, progress_bar, &header, &romfile_roms)
                .await;
        } else if CSO_EXTENSION == romfile_extension {
            if maxcso::get_version().await.is_err() {
                progress_bar.println("Please install maxcso");
                break;
            }
            result = romfile
                .as_common(&mut transaction)
                .await?
                .as_xso()
                .await?
                .check(&mut transaction, progress_bar, &header, &romfile_roms)
                .await;
        } else if NSZ_EXTENSION == romfile_extension {
            if nsz::get_version().await.is_err() {
                progress_bar.println("Please install nsz");
                break;
            }
            result = romfile
                .as_common(&mut transaction)
                .await?
                .as_nsz()?
                .check(&mut transaction, progress_bar, &header, &romfile_roms)
                .await;
        } else if RVZ_EXTENSION == romfile_extension {
            if dolphin::get_version().await.is_err() {
                progress_bar.println("Please install dolphin-tool");
                break;
            }
            result = romfile
                .as_common(&mut transaction)
                .await?
                .as_rvz()?
                .check(&mut transaction, progress_bar, &header, &romfile_roms)
                .await;
        } else if ZSO_EXTENSION == romfile_extension {
            if maxcso::get_version().await.is_err() {
                progress_bar.println("Please install maxcso");
                break;
            }
            result = romfile
                .as_common(&mut transaction)
                .await?
                .as_xso()
                .await?
                .check(&mut transaction, progress_bar, &header, &romfile_roms)
                .await;
        } else {
            result = romfile
                .as_common(&mut transaction)
                .await?
                .check(&mut transaction, progress_bar, &header, &romfile_roms)
                .await;
        }

        if result.is_err() {
            errors += 1;
            move_to_trash(&mut transaction, progress_bar, system, romfile).await?;
        } else if size {
            romfile
                .as_common(&mut transaction)
                .await?
                .update(&mut transaction, progress_bar, romfile.id)
                .await?;
        }
    }

    // update games and systems completion
    if errors > 0 {
        compute_system_completion(&mut transaction, progress_bar, system).await;
    }

    commit_transaction(transaction).await;

    Ok(())
}

async fn check_archive(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    header: &Option<Header>,
    romfile: &Romfile,
    roms: Vec<&Rom>,
) -> SimpleResult<()> {
    let archive_romfiles = romfile
        .as_common(connection)
        .await?
        .as_archive(progress_bar, None)
        .await?;
    if archive_romfiles.len() != roms.len() {
        bail!("Archive contains a different number of ROM files");
    }
    for archive_romfile in archive_romfiles {
        let rom = roms
            .iter()
            .find(|rom| rom.name == archive_romfile.path)
            .unwrap();
        archive_romfile
            .check(connection, progress_bar, header, &[rom])
            .await?;
    }
    Ok(())
}

async fn move_to_trash(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    romfile: &Romfile,
) -> SimpleResult<()> {
    let new_path = get_trash_directory(connection, Some(system)).await?.join(
        romfile
            .as_common(connection)
            .await?
            .path
            .file_name()
            .unwrap(),
    );
    romfile
        .as_common(connection)
        .await?
        .rename(progress_bar, &new_path, false)
        .await?
        .update(connection, progress_bar, romfile.id)
        .await?;
    Ok(())
}

#[cfg(test)]
mod test_cso;
#[cfg(test)]
mod test_iso_chd;
#[cfg(test)]
mod test_multiple_tracks_chd;
#[cfg(test)]
mod test_original;
#[cfg(test)]
mod test_original_crc_mismatch;
#[cfg(test)]
mod test_original_size_mismatch;
#[cfg(test)]
mod test_original_with_header;
#[cfg(test)]
mod test_rvz;
#[cfg(test)]
mod test_sevenzip;
#[cfg(test)]
mod test_sevenzip_with_header;
#[cfg(test)]
mod test_zip;
#[cfg(test)]
mod test_zso;

use super::config::*;
use super::database::*;
use super::download_dats::REDUMP_SYSTEM_URL;
use super::model::*;
use super::prompt::*;
use super::util::*;
use super::SimpleResult;
use async_std::fs::File;
use async_std::io::WriteExt;
use async_std::path::PathBuf;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use regex::Regex;
use sqlx::sqlite::SqliteConnection;
use std::collections::HashMap;

lazy_static! {
    pub static ref DISC_REGEX: Regex = Regex::new(r" \(Disc \d+\).*").unwrap();
}

pub fn subcommand() -> Command {
    Command::new("generate-playlists")
        .about("Generate M3U playlists for multi-disc games")
        .arg(
            Arg::new("ALL")
                .short('a')
                .long("all")
                .help("Generate playlists for all systems")
                .required(false)
                .action(ArgAction::SetTrue),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let systems = prompt_for_systems(
        connection,
        Some(REDUMP_SYSTEM_URL),
        false,
        matches.get_flag("ALL"),
    )
    .await?;
    for system in systems {
        progress_bar.println(format!("Processing \"{}\"", system.name));
        process_system(connection, progress_bar, &system).await?;
        progress_bar.println("");
    }
    Ok(())
}

async fn process_system(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
) -> SimpleResult<()> {
    let mut grouped_games: HashMap<String, Vec<Game>> = HashMap::new();
    find_games_with_romfiles_by_system_id(connection, system.id)
        .await
        .into_iter()
        .filter(|game| DISC_REGEX.is_match(&game.name))
        .for_each(|game| {
            let playlist_name = format!("{}.{}", DISC_REGEX.replace(&game.name, ""), M3U_EXTENSION);
            let group = grouped_games.entry(playlist_name).or_insert_with(Vec::new);
            group.push(game);
        });

    for (playlist_name, games) in grouped_games.into_iter() {
        let roms = find_roms_with_romfile_by_game_ids(
            connection,
            games
                .iter()
                .map(|game| game.id)
                .collect::<Vec<i64>>()
                .as_slice(),
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

        let mut existing_romfiles: Vec<&Romfile> = Vec::new();

        for extension in [
            CHD_EXTENSION,
            CSO_EXTENSION,
            CUE_EXTENSION,
            ISO_EXTENSION,
            RVZ_EXTENSION,
        ] {
            existing_romfiles = romfiles
                .iter()
                .filter(|romfile| romfile.path.ends_with(extension))
                .collect();
            if !existing_romfiles.is_empty() {
                break;
            }
        }

        if existing_romfiles.is_empty() {
            continue;
        }

        let mut playlist_path = PathBuf::from(&existing_romfiles.get(0).unwrap().path);
        playlist_path.set_file_name(&playlist_name);
        let mut playlist_file = File::create(&playlist_path)
            .await
            .expect("Failed to create M3U file");

        progress_bar.println(format!("Creating \"{}\"", &playlist_name));

        for romfile in existing_romfiles {
            writeln!(
                &mut playlist_file,
                "{}",
                PathBuf::from(&romfile.path)
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
            )
            .await
            .expect("Failed to write to M3U file");
        }

        let playlist_id =
            match find_romfile_by_path(connection, playlist_path.as_os_str().to_str().unwrap())
                .await
            {
                Some(playlist) => {
                    update_romfile(
                        connection,
                        playlist.id,
                        playlist_path.as_os_str().to_str().unwrap(),
                        playlist_path.metadata().await.unwrap().len(),
                    )
                    .await;
                    if playlist.path != playlist_path.as_os_str().to_str().unwrap() {
                        remove_file(progress_bar, &playlist.path, true).await?;
                    }
                    playlist.id
                }
                None => {
                    create_romfile(
                        connection,
                        playlist_path.as_os_str().to_str().unwrap(),
                        playlist_path.metadata().await.unwrap().len(),
                    )
                    .await
                }
            };
        for game in games {
            update_game_playlist(connection, game.id, playlist_id).await;
        }
    }
    Ok(())
}

#[cfg(test)]
mod test_iso;

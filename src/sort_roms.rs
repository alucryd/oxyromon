use super::config::*;
use super::database::*;
use super::model::*;
use super::progress::*;
use super::prompt::*;
use super::util::*;
use super::SimpleResult;
use async_std::path::{Path, PathBuf};
use async_std::stream::StreamExt;
use cfg_if::cfg_if;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use rayon::prelude::*;
use shiratsu_naming::naming::nointro::{NoIntroName, NoIntroToken};
use shiratsu_naming::naming::TokenizedName;
use shiratsu_naming::region::Region;
use sqlx::sqlite::SqliteConnection;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::ffi::OsString;
use std::time::Duration;

pub fn subcommand() -> Command {
    Command::new("sort-roms")
        .about("Sort ROM files according to region and version preferences")
        .arg(
            Arg::new("REGIONS_ALL")
                .short('r')
                .long("regions")
                .help("Set the regions to keep (unordered)")
                .required(false)
                .num_args(1..),
        )
        .arg(
            Arg::new("REGIONS_ONE")
                .short('g')
                .long("1g1r")
                .help("Set the 1G1R regions to keep (ordered)")
                .required(false)
                .num_args(1..),
        )
        .arg(
            Arg::new("WANTED")
                .short('w')
                .long("wanted")
                .help("Show wanted games")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("ALL")
                .short('a')
                .long("all")
                .help("Sort all systems")
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
) -> SimpleResult<()> {
    let systems = prompt_for_systems(connection, None, false, matches.get_flag("ALL")).await?;

    progress_bar.enable_steady_tick(Duration::from_millis(100));

    let all_regions = get_regions(connection, matches, "REGIONS_ALL").await;
    let one_regions = get_regions(connection, matches, "REGIONS_ONE").await;
    let ignored_releases = get_list(connection, "DISCARD_RELEASES").await;
    let ignored_flags = get_list(connection, "DISCARD_FLAGS").await;

    for system in systems {
        sort_system(
            connection,
            matches,
            progress_bar,
            &system,
            &all_regions,
            &one_regions,
            &ignored_releases
                .iter()
                .map(String::as_str)
                .collect::<Vec<&str>>(),
            &ignored_flags
                .iter()
                .map(String::as_str)
                .collect::<Vec<&str>>(),
        )
        .await?;

        progress_bar.println("");
    }

    Ok(())
}

pub async fn get_regions(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    key: &str,
) -> Vec<Region> {
    let all_regions: Vec<String> = if matches.contains_id(key) {
        let mut regions: Vec<String> = matches
            .get_many::<String>(key)
            .unwrap()
            .map(String::to_owned)
            .collect();
        regions.dedup();
        regions
    } else {
        get_list(connection, key).await
    };
    all_regions
        .into_iter()
        .flat_map(|r| Region::try_from_tosec_region(&r).expect("Failed to parse region code"))
        .collect()
}

async fn sort_system(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
    system: &System,
    all_regions: &[Region],
    one_regions: &[Region],
    ignored_releases: &[&str],
    ignored_flags: &[&str],
) -> SimpleResult<()> {
    progress_bar.println(&format!("Processing \"{}\"", system.name));

    let mut games: Vec<Game>;
    let mut all_regions_games: Vec<Game> = Vec::new();
    let mut one_region_games: Vec<Game> = Vec::new();
    let mut ignored_games: Vec<Game> = Vec::new();
    let mut wanted_games: Vec<Game> = Vec::new();
    let mut romfile_moves: Vec<(&Romfile, String)> = Vec::new();

    let romfiles = find_romfiles_by_system_id(connection, system.id).await;
    let romfiles_by_id: HashMap<i64, Romfile> = romfiles
        .into_iter()
        .map(|romfile| (romfile.id, romfile))
        .collect();

    // 1G1R mode
    if !system.arcade && !one_regions.is_empty() {
        let parent_games = find_parent_games_by_system_id(connection, system.id).await;
        let clone_games = find_clone_games_by_system_id(connection, system.id).await;

        let mut clone_games_by_parent_id: HashMap<i64, Vec<Game>> = HashMap::new();
        clone_games.into_iter().for_each(|game| {
            let group = clone_games_by_parent_id
                .entry(game.parent_id.unwrap())
                .or_insert_with(Vec::new);
            group.push(game);
        });

        for parent_game in parent_games {
            if clone_games_by_parent_id.contains_key(&parent_game.id) {
                games = clone_games_by_parent_id.remove(&parent_game.id).unwrap();
                // put newer releases first
                games.sort_by(sort_games_by_version_or_name_desc);
            } else {
                games = Vec::new();
            }
            games.insert(0, parent_game);

            // trim ignored games
            if !ignored_releases.is_empty() || !ignored_flags.is_empty() {
                let (mut left_games, right_games) =
                    trim_ignored_games(games, ignored_releases, ignored_flags, system.arcade);
                ignored_games.append(&mut left_games);
                games = right_games;
            }

            // find the one game we want to keep, if any
            for region in one_regions {
                let i = games.iter().position(|game| {
                    game.complete
                        && Region::try_from_tosec_region(&game.regions)
                            .unwrap_or_default()
                            .contains(region)
                });
                if i.is_some() {
                    one_region_games.push(games.remove(i.unwrap()));
                    break;
                }
            }

            // go through the remaining games
            while !games.is_empty() {
                let game = games.remove(0);
                let region_in_all_regions = all_regions.iter().any(|region| {
                    Region::try_from_tosec_region(&game.regions)
                        .unwrap_or_default()
                        .contains(region)
                });
                if region_in_all_regions {
                    if game.complete {
                        all_regions_games.push(game);
                    } else {
                        wanted_games.push(game);
                    }
                } else {
                    ignored_games.push(game);
                }
            }
        }
    // Regions mode
    } else if !system.arcade && !all_regions.is_empty() {
        games = find_games_by_system_id(connection, system.id).await;

        // trim ignored games
        if !ignored_releases.is_empty() || !ignored_flags.is_empty() {
            let (mut left_games, right_games) =
                trim_ignored_games(games, ignored_releases, ignored_flags, system.arcade);
            ignored_games.append(&mut left_games);
            games = right_games;
        }

        for game in games {
            let region_in_all_regions = all_regions.iter().any(|region| {
                Region::try_from_tosec_region(&game.regions)
                    .unwrap_or_default()
                    .contains(region)
            });
            if region_in_all_regions {
                if game.complete {
                    all_regions_games.push(game);
                } else {
                    wanted_games.push(game);
                }
            } else {
                ignored_games.push(game);
            }
        }
    } else {
        games = find_games_by_system_id(connection, system.id).await;

        // trim ignored games
        if !ignored_releases.is_empty() || !ignored_flags.is_empty() {
            let (mut left_games, right_games) =
                trim_ignored_games(games, ignored_releases, ignored_flags, system.arcade);
            ignored_games.append(&mut left_games);
            games = right_games;
        }

        for game in games {
            if game.complete {
                all_regions_games.push(game);
            } else {
                wanted_games.push(game)
            }
        }
    }

    if matches.get_flag("WANTED") {
        let mut wanted_roms: Vec<Rom> = find_roms_without_romfile_by_game_ids(
            connection,
            &wanted_games
                .par_iter()
                .map(|game| game.id)
                .collect::<Vec<i64>>(),
        )
        .await;

        if !wanted_roms.is_empty() {
            progress_bar.println("Wanted:");
            wanted_roms.sort_by_key(|rom| rom.game_id);
            for rom in wanted_roms {
                let game = wanted_games
                    .iter()
                    .find(|&game| game.id == rom.game_id)
                    .unwrap();
                progress_bar.println(&format!(
                    "{} ({}) [{}]",
                    rom.name,
                    game.name,
                    rom.crc.as_ref().unwrap()
                ));
            }
        } else {
            progress_bar.println("No wanted ROMs");
        }
    }

    let system_directory = get_system_directory(connection, progress_bar, system).await?;
    let one_region_directory = get_one_region_directory(connection, progress_bar, system).await?;
    let trash_directory = get_trash_directory(connection, progress_bar, system).await?;

    let mut transaction = begin_transaction(connection).await;

    let mut changes = 0;

    // process all region games
    changes += update_games_sorting(
        &mut transaction,
        &all_regions_games
            .iter()
            .map(|game| game.id)
            .collect::<Vec<i64>>(),
        Sorting::AllRegions,
    )
    .await;
    romfile_moves.append(
        &mut sort_games(
            &mut transaction,
            system,
            all_regions_games,
            &system_directory,
            &romfiles_by_id,
        )
        .await?,
    );

    // process one region games
    changes += update_games_sorting(
        &mut transaction,
        &one_region_games
            .iter()
            .map(|game| game.id)
            .collect::<Vec<i64>>(),
        Sorting::OneRegion,
    )
    .await;
    romfile_moves.append(
        &mut sort_games(
            &mut transaction,
            system,
            one_region_games,
            &one_region_directory,
            &romfiles_by_id,
        )
        .await?,
    );

    // process wanted games
    changes += update_games_sorting(
        &mut transaction,
        &wanted_games
            .iter()
            .map(|game| game.id)
            .collect::<Vec<i64>>(),
        Sorting::AllRegions,
    )
    .await;
    romfile_moves.append(
        &mut sort_games(
            &mut transaction,
            system,
            wanted_games,
            &system_directory,
            &romfiles_by_id,
        )
        .await?,
    );

    // process ignored games
    changes += update_games_sorting(
        &mut transaction,
        &ignored_games
            .iter()
            .map(|game| game.id)
            .collect::<Vec<i64>>(),
        Sorting::Ignored,
    )
    .await;
    romfile_moves.append(
        &mut sort_games(
            &mut transaction,
            system,
            ignored_games,
            &trash_directory,
            &romfiles_by_id,
        )
        .await?,
    );

    if !romfile_moves.is_empty() {
        // sort moves and print a summary
        romfile_moves.sort_by(|a, b| a.1.cmp(&b.1));
        romfile_moves.dedup_by(|a, b| a.1 == b.1);

        progress_bar.println("Summary:");
        for romfile_move in &romfile_moves {
            progress_bar.println(&format!(
                "{:?} -> \"{}\"",
                Path::new(&romfile_move.0.path).file_name().unwrap(),
                romfile_move.1
            ));
        }

        // prompt user for confirmation
        if matches.get_flag("YES") || confirm(true)? {
            for romfile_move in romfile_moves {
                rename_file(progress_bar, &romfile_move.0.path, &romfile_move.1, true).await?;
                update_romfile(
                    &mut transaction,
                    romfile_move.0.id,
                    &romfile_move.1,
                    romfile_move.0.size as u64,
                )
                .await;
                // delete empty directories
                let mut directory = Path::new(&romfile_move.0.path).parent().unwrap();
                while directory.read_dir().await.unwrap().next().await.is_none() {
                    if directory == system_directory {
                        break;
                    } else {
                        remove_directory(progress_bar, &directory, true).await?;
                        directory = directory.parent().unwrap();
                    }
                }
            }
            commit_transaction(transaction).await;
        } else {
            rollback_transaction(transaction).await;
        }
    } else {
        commit_transaction(transaction).await;
    }

    // update games and systems completion
    if changes > 0 {
        progress_bar.set_style(get_none_progress_style());
        progress_bar.enable_steady_tick(Duration::from_millis(100));
        progress_bar.set_message("Computing system completion");
        update_games_by_system_id_mark_incomplete(connection, system.id).await;
        cfg_if! {
            if #[cfg(feature = "ird")] {
                update_jbfolder_games_by_system_id_mark_incomplete(connection, system.id).await;
            }
        }
        update_system_mark_complete(connection, system.id).await;
        update_system_mark_incomplete(connection, system.id).await;
        progress_bar.set_message("");
    }

    Ok(())
}

async fn sort_games<'a, P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    system: &System,
    games: Vec<Game>,
    directory: &P,
    romfiles_by_id: &'a HashMap<i64, Romfile>,
) -> SimpleResult<Vec<(&'a Romfile, String)>> {
    let mut romfile_moves: Vec<(&Romfile, String)> = Vec::new();

    let roms = find_roms_with_romfile_by_game_ids(
        connection,
        games
            .iter()
            .map(|game| game.id)
            .collect::<Vec<i64>>()
            .as_slice(),
    )
    .await;

    let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
    roms.into_iter().for_each(|rom| {
        let group = roms_by_game_id.entry(rom.game_id).or_insert_with(Vec::new);
        group.push(rom);
    });

    for game in games {
        let roms = roms_by_game_id.get(&game.id);
        let roms = match roms {
            Some(roms) => roms,
            None => continue,
        };
        let rom_count = roms.len();
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let new_path = String::from(
                compute_new_path(system, &game, rom, romfile, rom_count, directory)
                    .await?
                    .as_os_str()
                    .to_str()
                    .unwrap(),
            );
            if romfile.path != new_path {
                romfile_moves.push((romfile, new_path))
            }
        }
    }

    Ok(romfile_moves)
}

fn trim_ignored_games(
    games: Vec<Game>,
    ignored_releases: &[&str],
    ignored_flags: &[&str],
    arcade: bool,
) -> (Vec<Game>, Vec<Game>) {
    // TODO: use drain_filter when it hits stable
    if arcade {
        games.into_iter().partition(|game| {
            let comment = match game.comment.as_ref() {
                Some(comment) => comment,
                None => "",
            };
            for ignored_release in ignored_releases {
                if comment.contains(ignored_release) {
                    return true;
                }
            }
            false
        })
    } else {
        games.into_iter().partition(|game| {
            if let Ok(name) = NoIntroName::try_parse(&game.name) {
                for token in name.iter() {
                    if let NoIntroToken::Release(release, _) = token {
                        if ignored_releases.contains(release) {
                            return true;
                        }
                    }
                    if let NoIntroToken::Flag(_, flags) = token {
                        for flag in flags.split(", ") {
                            if ignored_flags.contains(&flag) {
                                return true;
                            }
                        }
                    }
                }
            }
            false
        })
    }
}

fn sort_games_by_version_or_name_desc(game_a: &Game, game_b: &Game) -> Ordering {
    let mut version_a = None;
    let mut version_b = None;
    if let Ok(name) = NoIntroName::try_parse(&game_a.name) {
        for token in name.iter() {
            if let NoIntroToken::Version(version) = token {
                version_a = Some(version.to_owned());
            }
        }
    }
    if let Ok(name) = NoIntroName::try_parse(&game_b.name) {
        for token in name.iter() {
            if let NoIntroToken::Version(version) = token {
                version_b = Some(version.to_owned());
            }
        }
    }
    if let (Some(version_a), Some(version_b)) = (version_a.as_ref(), version_b.as_ref()) {
        return version_b.partial_cmp(version_a).unwrap();
    }
    if version_a.is_some() {
        return Ordering::Less;
    }
    if version_b.is_some() {
        return Ordering::Greater;
    }
    game_b.name.partial_cmp(&game_a.name).unwrap()
}

async fn compute_new_path<P: AsRef<Path>>(
    system: &System,
    game: &Game,
    rom: &Rom,
    romfile: &Romfile,
    rom_count: usize,
    directory: &P,
) -> SimpleResult<PathBuf> {
    let romfile_path = Path::new(&romfile.path);
    let romfile_extension = romfile_path
        .extension()
        .unwrap_or(&OsString::new())
        .to_str()
        .unwrap()
        .to_lowercase();
    let mut new_romfile_path: PathBuf;

    if ARCHIVE_EXTENSIONS.contains(&romfile_extension.as_str()) {
        new_romfile_path = directory.as_ref().join(match rom_count {
            1 => {
                let rom_extension = Path::new(&rom.name)
                    .extension()
                    .unwrap_or(&OsString::new())
                    .to_str()
                    .unwrap()
                    .to_lowercase();
                if system.arcade
                    || game.jbfolder
                    || PS3_EXTENSIONS.contains(&rom_extension.as_str())
                {
                    format!("{}.{}", &game.name, &romfile_extension)
                } else {
                    format!("{}.{}", &rom.name, &romfile_extension)
                }
            }
            _ => format!("{}.{}", &game.name, &romfile_extension),
        });
    } else if romfile_extension == CHD_EXTENSION {
        if rom_count == 2 {
            new_romfile_path = directory.as_ref().join(&rom.name);
            new_romfile_path.set_extension(&romfile_extension);
        } else {
            new_romfile_path = directory
                .as_ref()
                .join(format!("{}.{}", &game.name, &romfile_extension));
        }
    } else if romfile_extension == CSO_EXTENSION || romfile_extension == RVZ_EXTENSION {
        new_romfile_path = directory.as_ref().join(&rom.name);
        new_romfile_path.set_extension(&romfile_extension);
    } else if system.arcade || game.jbfolder {
        new_romfile_path = directory.as_ref().join(&game.name).join(&rom.name);
    } else if PS3_EXTENSIONS.contains(&romfile_extension.as_str()) {
        new_romfile_path = directory
            .as_ref()
            .join(format!("{}.{}", &game.name, &romfile_extension));
    } else {
        new_romfile_path = directory.as_ref().join(&rom.name);
    }
    Ok(new_romfile_path)
}

#[cfg(test)]
mod test_all_regions_from_db;
#[cfg(test)]
mod test_all_regions_from_matches;
#[cfg(test)]
mod test_one_region_from_db;
#[cfg(test)]
mod test_one_region_from_matches;
#[cfg(test)]
mod test_order_revision_vs_revision;
#[cfg(test)]
mod test_order_revision_vs_vanilla;
#[cfg(test)]
mod test_order_vanilla_vs_revision;
#[cfg(test)]
mod test_path_archive_multiple_files;
#[cfg(test)]
mod test_path_archive_single_file;
#[cfg(all(test, feature = "chd"))]
mod test_path_chd_multiple_tracks;
#[cfg(all(test, feature = "chd"))]
mod test_path_chd_single_track;
#[cfg(all(test, feature = "cso"))]
mod test_path_cso;
#[cfg(test)]
mod test_path_original;
#[cfg(test)]
mod test_sort;
#[cfg(test)]
mod test_sort_1g1r;
#[cfg(test)]
mod test_sort_1g1r_discard_asia_and_beta;
#[cfg(test)]
mod test_sort_1g1r_without_parent_clone;
#[cfg(test)]
mod test_sort_discard_asia;
#[cfg(test)]
mod test_sort_discard_asia_and_beta;
#[cfg(test)]
mod test_sort_discard_beta;
#[cfg(test)]
mod test_trim_ignored;

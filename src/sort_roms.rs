use super::config::*;
use super::database::*;
use super::model::*;
use super::progress::*;
use super::prompt::*;
use super::util::*;
use super::SimpleResult;
use async_std::path::{Path, PathBuf};
use clap::{App, Arg, ArgMatches};
use indicatif::ProgressBar;
use rayon::prelude::*;
use shiratsu_naming::naming::nointro::{NoIntroName, NoIntroToken};
use shiratsu_naming::naming::TokenizedName;
use shiratsu_naming::region::Region;
use sqlx::sqlite::SqliteConnection;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::ffi::OsString;

pub fn subcommand<'a>() -> App<'a> {
    App::new("sort-roms")
        .about("Sort ROM files according to region and version preferences")
        .arg(
            Arg::new("REGIONS_ALL")
                .short('r')
                .long("regions")
                .help("Set the regions to keep (unordered)")
                .required(false)
                .takes_value(true)
                .multiple_values(true),
        )
        .arg(
            Arg::new("REGIONS_ONE")
                .short('g')
                .long("1g1r")
                .help("Set the 1G1R regions to keep (ordered)")
                .required(false)
                .takes_value(true)
                .multiple_values(true),
        )
        .arg(
            Arg::new("MISSING")
                .short('m')
                .long("missing")
                .help("Show missing games")
                .required(false),
        )
        .arg(
            Arg::new("ALL")
                .short('a')
                .long("all")
                .help("Sort all systems")
                .required(false),
        )
        .arg(
            Arg::new("YES")
                .short('y')
                .long("yes")
                .help("Automatically say yes to prompts")
                .required(false),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let systems = prompt_for_systems(connection, None, false, matches.is_present("ALL")).await?;

    progress_bar.enable_steady_tick(100);

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
    let all_regions: Vec<String> = if matches.is_present(key) {
        let mut regions: Vec<String> = matches.values_of(key).unwrap().map(String::from).collect();
        regions.dedup();
        regions
    } else {
        get_list(connection, key).await
    };
    all_regions
        .into_iter()
        .map(|r| Region::try_from_tosec_region(&r).expect("Failed to parse region code"))
        .flatten()
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
    let mut missing_games: Vec<Game> = Vec::new();
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
                .or_insert(vec![]);
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
                        missing_games.push(game);
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
                    missing_games.push(game);
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
                missing_games.push(game)
            }
        }
    }

    if matches.is_present("MISSING") {
        let mut missing_roms: Vec<Rom> = find_roms_without_romfile_by_game_ids(
            connection,
            &missing_games
                .par_iter()
                .map(|game| game.id)
                .collect::<Vec<i64>>(),
        )
        .await;

        if !missing_roms.is_empty() {
            progress_bar.println("Missing:");
            missing_roms.sort_by_key(|rom| rom.game_id);
            for rom in missing_roms {
                let game = missing_games
                    .iter()
                    .find(|&game| game.id == rom.game_id)
                    .unwrap();
                progress_bar.println(&format!("{} ({}) [{}]", rom.name, game.name, rom.crc));
            }
        } else {
            progress_bar.println("No missing ROMs");
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
            progress_bar,
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
            progress_bar,
            system,
            one_region_games,
            &one_region_directory,
            &romfiles_by_id,
        )
        .await?,
    );

    // process missing games
    changes += update_games_sorting(
        &mut transaction,
        &missing_games
            .iter()
            .map(|game| game.id)
            .collect::<Vec<i64>>(),
        Sorting::AllRegions,
    )
    .await;
    romfile_moves.append(
        &mut sort_games(
            &mut transaction,
            progress_bar,
            system,
            missing_games,
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
            progress_bar,
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
        if matches.is_present("YES") || confirm(true)? {
            for romfile_move in romfile_moves {
                rename_file(progress_bar, &romfile_move.0.path, &romfile_move.1, false).await?;
                update_romfile(
                    &mut transaction,
                    romfile_move.0.id,
                    &romfile_move.1,
                    romfile_move.0.size as u64,
                )
                .await;
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
        progress_bar.enable_steady_tick(100);
        progress_bar.set_message("Computing system completion");
        update_games_by_system_id_mark_incomplete(connection, system.id).await;
        update_system_mark_complete(connection, system.id).await;
        update_system_mark_incomplete(connection, system.id).await;
    }

    Ok(())
}

async fn sort_games<'a, P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
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
        let group = roms_by_game_id.entry(rom.game_id).or_insert(vec![]);
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
                compute_new_path(
                    progress_bar,
                    system,
                    &game,
                    rom,
                    romfile,
                    rom_count,
                    directory,
                )
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
    progress_bar: &ProgressBar,
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
                if system.arcade {
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
    } else if romfile_extension == CSO_EXTENSION {
        new_romfile_path = directory.as_ref().join(&rom.name);
        new_romfile_path.set_extension(&romfile_extension);
    } else if romfile_extension == RVZ_EXTENSION {
        new_romfile_path = directory.as_ref().join(&rom.name);
        new_romfile_path.set_extension(&romfile_extension);
    } else {
        new_romfile_path = match system.arcade {
            true => {
                create_directory(progress_bar, &directory.as_ref().join(&game.name), true).await?;
                directory.as_ref().join(&game.name).join(&rom.name)
            }
            false => directory.as_ref().join(&rom.name),
        };
    }
    Ok(new_romfile_path)
}

#[cfg(test)]
mod test {
    use super::super::database::*;
    use super::super::import_dats;
    use super::super::import_roms;
    use super::super::util::*;
    use super::*;
    use async_std::fs;
    use tempfile::{NamedTempFile, TempDir};

    #[async_std::test]
    async fn test_compute_regions_all_should_get_from_matches() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let key = "REGIONS_ALL";

        add_to_list(&mut connection, key, "US").await;
        let matches = subcommand().get_matches_from(&["sort-roms", "-y", "-r", "EU"]);

        // when
        let all_regions = get_regions(&mut connection, &matches, key).await;

        // then
        assert_eq!(all_regions.len(), 1);
        assert_eq!(all_regions.get(0).unwrap(), &Region::Europe);
    }

    #[async_std::test]
    async fn test_compute_regions_all_should_get_from_db() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let key = "REGIONS_ALL";

        add_to_list(&mut connection, key, "US").await;
        let matches = subcommand().get_matches_from(&["sort-roms", "-y"]);

        // when
        let all_regions = get_regions(&mut connection, &matches, key).await;

        // then
        assert_eq!(all_regions.len(), 1);
        assert_eq!(all_regions.get(0).unwrap(), &Region::UnitedStates);
    }

    #[async_std::test]
    async fn test_compute_regions_one_should_get_from_matches() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let key = "REGIONS_ONE";

        add_to_list(&mut connection, key, "US").await;
        let matches = subcommand().get_matches_from(&["sort-roms", "-y", "-g", "EU"]);

        // when
        let all_regions = get_regions(&mut connection, &matches, key).await;

        // then
        assert_eq!(all_regions.len(), 1);
        assert_eq!(all_regions.get(0).unwrap(), &Region::Europe);
    }

    #[async_std::test]
    async fn test_compute_regions_one_should_get_from_db() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let key = "REGIONS_ONE";

        add_to_list(&mut connection, key, "US").await;
        let matches = subcommand().get_matches_from(&["sort-roms", "-y"]);

        // when
        let all_regions = get_regions(&mut connection, &matches, key).await;

        // then
        assert_eq!(all_regions.len(), 1);
        assert_eq!(all_regions.get(0).unwrap(), &Region::UnitedStates);
    }

    #[async_std::test]
    async fn test_trim_ignored_games_should_discard_ignored_games() {
        // given
        let games = vec![
            Game {
                id: 1,
                name: String::from("Game (USA)"),
                description: String::from(""),
                comment: None,
                bios: false,
                regions: String::from(""),
                sorting: Sorting::AllRegions as i64,
                complete: true,
                system_id: 1,
                parent_id: None,
                bios_id: None,
            },
            Game {
                id: 2,
                name: String::from("Game (USA) (Beta)"),
                description: String::from(""),
                comment: None,
                bios: false,
                regions: String::from(""),
                sorting: Sorting::AllRegions as i64,
                complete: true,
                system_id: 1,
                parent_id: None,
                bios_id: None,
            },
            Game {
                id: 3,
                name: String::from("Game (USA) (Beta 1)"),
                description: String::from(""),
                comment: None,
                bios: false,
                regions: String::from(""),
                sorting: Sorting::AllRegions as i64,
                complete: true,
                system_id: 1,
                parent_id: None,
                bios_id: None,
            },
            Game {
                id: 4,
                name: String::from("Game (USA) (Virtual Console, Switch Online)"),
                description: String::from(""),
                comment: None,
                bios: false,
                regions: String::from(""),
                sorting: Sorting::AllRegions as i64,
                complete: true,
                system_id: 1,
                parent_id: None,
                bios_id: None,
            },
        ];

        let ignored_releases = vec!["Beta"];
        let ignored_flags = vec!["Virtual Console"];

        // when
        let (ignored_games, regular_games) =
            trim_ignored_games(games, &ignored_releases, &ignored_flags, false);

        // then
        assert_eq!(ignored_games.len(), 3);
        assert_eq!(regular_games.len(), 1);
        assert_eq!(regular_games.get(0).unwrap().name, "Game (USA)")
    }

    #[async_std::test]
    async fn test_sort_games_by_version_or_name_between_two_revisions() {
        // given
        let game_a = Game {
            id: 1,
            name: String::from("Game (USA) (Rev 1)"),
            description: String::from(""),
            comment: None,
            bios: false,
            regions: String::from(""),
            sorting: Sorting::AllRegions as i64,
            complete: true,
            system_id: 1,
            parent_id: None,
            bios_id: None,
        };
        let game_b = Game {
            id: 1,
            name: String::from("Game (USA) (Rev 2)"),
            description: String::from(""),
            comment: None,
            bios: false,
            regions: String::from(""),
            sorting: Sorting::AllRegions as i64,
            complete: true,
            system_id: 1,
            parent_id: None,
            bios_id: None,
        };

        // when
        let ordering = sort_games_by_version_or_name_desc(&game_a, &game_b);

        // then
        assert_eq!(ordering, Ordering::Greater);
    }

    #[async_std::test]
    async fn test_sort_games_by_version_or_name_between_vanilla_and_revision() {
        // given
        let game_a = Game {
            id: 1,
            name: String::from("Game (USA)"),
            description: String::from(""),
            comment: None,
            bios: false,
            regions: String::from(""),
            sorting: Sorting::AllRegions as i64,
            complete: true,
            system_id: 1,
            parent_id: None,
            bios_id: None,
        };
        let game_b = Game {
            id: 1,
            name: String::from("Game (USA) (Rev 2)"),
            description: String::from(""),
            comment: None,
            bios: false,
            regions: String::from(""),
            sorting: Sorting::AllRegions as i64,
            complete: true,
            system_id: 1,
            parent_id: None,
            bios_id: None,
        };

        // when
        let ordering = sort_games_by_version_or_name_desc(&game_a, &game_b);

        // then
        assert_eq!(ordering, Ordering::Greater);
    }

    #[async_std::test]
    async fn test_sort_games_by_version_or_name_between_revision_and_vanilla() {
        // given
        let game_a = Game {
            id: 1,
            name: String::from("Game (USA) (Rev 2"),
            description: String::from(""),
            comment: None,
            bios: false,
            regions: String::from(""),
            sorting: Sorting::AllRegions as i64,
            complete: true,
            system_id: 1,
            parent_id: None,
            bios_id: None,
        };
        let game_b = Game {
            id: 1,
            name: String::from("Game (USA)"),
            description: String::from(""),
            comment: None,
            bios: false,
            regions: String::from(""),
            sorting: Sorting::AllRegions as i64,
            complete: true,
            system_id: 1,
            parent_id: None,
            bios_id: None,
        };

        // when
        let ordering = sort_games_by_version_or_name_desc(&game_a, &game_b);

        // then
        assert_eq!(ordering, Ordering::Less);
    }

    #[async_std::test]
    async fn test_compute_new_path_archive_single_file() {
        // given
        let progress_bar = ProgressBar::hidden();
        let test_directory = Path::new("test");
        let system = System {
            id: 1,
            name: String::from("Test System"),
            description: String::from(""),
            version: String::from(""),
            url: Some(String::from("")),
            arcade: false,
            merging: Merging::Split as i64,
            complete: false,
        };
        let game = Game {
            id: 1,
            name: String::from("game name"),
            description: String::from(""),
            comment: None,
            bios: false,
            regions: String::from(""),
            sorting: Sorting::AllRegions as i64,
            complete: false,
            system_id: 1,
            parent_id: None,
            bios_id: None,
        };
        let rom = Rom {
            id: 1,
            name: String::from("rom name.rom"),
            bios: false,
            size: 1,
            crc: String::from(""),
            md5: Some(String::from("")),
            sha1: Some(String::from("")),
            rom_status: None,
            game_id: 1,
            romfile_id: Some(1),
            parent_id: None,
        };
        let romfile = Romfile {
            id: 1,
            path: String::from("romfile.7z"),
            size: 0,
        };

        // when
        let path = compute_new_path(
            &progress_bar,
            &system,
            &game,
            &rom,
            &romfile,
            1,
            &test_directory,
        )
        .await
        .unwrap();

        // then
        assert_eq!(path, test_directory.join("rom name.rom.7z"));
    }

    #[async_std::test]
    async fn test_compute_new_path_archive_multiple_files() {
        // given
        let progress_bar = ProgressBar::hidden();
        let test_directory = Path::new("test");
        let system = System {
            id: 1,
            name: String::from("Test System"),
            description: String::from(""),
            version: String::from(""),
            url: Some(String::from("")),
            arcade: false,
            merging: Merging::Split as i64,
            complete: false,
        };
        let game = Game {
            id: 1,
            name: String::from("game name"),
            description: String::from(""),
            comment: None,
            bios: false,
            regions: String::from(""),
            sorting: Sorting::AllRegions as i64,
            complete: false,
            system_id: 1,
            parent_id: None,
            bios_id: None,
        };
        let rom = Rom {
            id: 1,
            name: String::from("rom name.rom"),
            bios: false,
            size: 1,
            crc: String::from(""),
            md5: Some(String::from("")),
            sha1: Some(String::from("")),
            rom_status: None,
            game_id: 1,
            romfile_id: Some(1),
            parent_id: None,
        };
        let romfile = Romfile {
            id: 1,
            path: String::from("romfile.7z"),
            size: 0,
        };

        // when
        let path = compute_new_path(
            &progress_bar,
            &system,
            &game,
            &rom,
            &romfile,
            2,
            &test_directory,
        )
        .await
        .unwrap();

        // then
        assert_eq!(path, test_directory.join("game name.7z"));
    }

    #[async_std::test]
    async fn test_compute_new_path_chd_single_file() {
        // given
        let progress_bar = ProgressBar::hidden();
        let test_directory = Path::new("test");
        let system = System {
            id: 1,
            name: String::from("Test System"),
            description: String::from(""),
            version: String::from(""),
            url: Some(String::from("")),
            arcade: false,
            merging: Merging::Split as i64,
            complete: false,
        };
        let game = Game {
            id: 1,
            name: String::from("game name"),
            description: String::from(""),
            comment: None,
            bios: false,
            regions: String::from(""),
            sorting: Sorting::AllRegions as i64,
            complete: false,
            system_id: 1,
            parent_id: None,
            bios_id: None,
        };
        let rom = Rom {
            id: 1,
            name: String::from("rom name.bin"),
            bios: false,
            size: 1,
            crc: String::from(""),
            md5: Some(String::from("")),
            sha1: Some(String::from("")),
            rom_status: None,
            game_id: 1,
            romfile_id: Some(1),
            parent_id: None,
        };
        let romfile = Romfile {
            id: 1,
            path: String::from("romfile.chd"),
            size: 0,
        };

        // when
        let path = compute_new_path(
            &progress_bar,
            &system,
            &game,
            &rom,
            &romfile,
            2,
            &test_directory,
        )
        .await
        .unwrap();

        // then
        assert_eq!(path, test_directory.join("rom name.chd"));
    }

    #[async_std::test]
    async fn test_compute_new_path_chd_multiple_files() {
        // given
        let progress_bar = ProgressBar::hidden();
        let test_directory = Path::new("test");
        let system = System {
            id: 1,
            name: String::from("Test System"),
            description: String::from(""),
            version: String::from(""),
            url: Some(String::from("")),
            arcade: false,
            merging: Merging::Split as i64,
            complete: false,
        };
        let game = Game {
            id: 1,
            name: String::from("game name"),
            description: String::from(""),
            comment: None,
            bios: false,
            regions: String::from(""),
            sorting: Sorting::AllRegions as i64,
            complete: false,
            system_id: 1,
            parent_id: None,
            bios_id: None,
        };
        let rom = Rom {
            id: 1,
            name: String::from("rom name.bin"),
            bios: false,
            size: 1,
            crc: String::from(""),
            md5: Some(String::from("")),
            sha1: Some(String::from("")),
            rom_status: None,
            game_id: 1,
            romfile_id: Some(1),
            parent_id: None,
        };
        let romfile = Romfile {
            id: 1,
            path: String::from("romfile.chd"),
            size: 0,
        };

        // when
        let path = compute_new_path(
            &progress_bar,
            &system,
            &game,
            &rom,
            &romfile,
            3,
            &test_directory,
        )
        .await
        .unwrap();

        // then
        assert_eq!(path, test_directory.join("game name.chd"));
    }

    #[async_std::test]
    async fn test_compute_new_path_cso() {
        // given
        let progress_bar = ProgressBar::hidden();
        let test_directory = Path::new("test");
        let system = System {
            id: 1,
            name: String::from("Test System"),
            description: String::from(""),
            version: String::from(""),
            url: Some(String::from("")),
            arcade: false,
            merging: Merging::Split as i64,
            complete: false,
        };
        let game = Game {
            id: 1,
            name: String::from("game name"),
            description: String::from(""),
            comment: None,
            bios: false,
            regions: String::from(""),
            sorting: Sorting::AllRegions as i64,
            complete: false,
            system_id: 1,
            parent_id: None,
            bios_id: None,
        };
        let rom = Rom {
            id: 1,
            name: String::from("rom name.iso"),
            bios: false,
            size: 1,
            crc: String::from(""),
            md5: Some(String::from("")),
            sha1: Some(String::from("")),
            rom_status: None,
            game_id: 1,
            romfile_id: Some(1),
            parent_id: None,
        };
        let romfile = Romfile {
            id: 1,
            path: String::from("romfile.cso"),
            size: 0,
        };

        // when
        let path = compute_new_path(
            &progress_bar,
            &system,
            &game,
            &rom,
            &romfile,
            1,
            &test_directory,
        )
        .await
        .unwrap();

        // then
        assert_eq!(path, test_directory.join("rom name.cso"));
    }

    #[async_std::test]
    async fn test_compute_new_path_other() {
        // given
        let progress_bar = ProgressBar::hidden();
        let test_directory = Path::new("test");
        let system = System {
            id: 1,
            name: String::from("Test System"),
            description: String::from(""),
            version: String::from(""),
            url: Some(String::from("")),
            arcade: false,
            merging: Merging::Split as i64,
            complete: false,
        };
        let game = Game {
            id: 1,
            name: String::from("game name"),
            description: String::from(""),
            comment: None,
            bios: false,
            regions: String::from(""),
            sorting: Sorting::AllRegions as i64,
            complete: false,
            system_id: 1,
            parent_id: None,
            bios_id: None,
        };
        let rom = Rom {
            id: 1,
            name: String::from("rom name.rom"),
            bios: false,
            size: 1,
            crc: String::from(""),
            md5: Some(String::from("")),
            sha1: Some(String::from("")),
            rom_status: None,
            game_id: 1,
            romfile_id: Some(1),
            parent_id: None,
        };
        let romfile = Romfile {
            id: 1,
            path: String::from("romfile.rom"),
            size: 0,
        };

        // when
        let path = compute_new_path(
            &progress_bar,
            &system,
            &game,
            &rom,
            &romfile,
            1,
            &test_directory,
        )
        .await
        .unwrap();

        // then
        assert_eq!(path, test_directory.join("rom name.rom"));
    }

    #[async_std::test]
    async fn test_sort_system_keep_all() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));

        let matches = import_dats::subcommand()
            .get_matches_from(&["import-dats", "test/Test System (20200721).dat"]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let romfile_names = vec![
            "Test Game (Asia).rom",
            "Test Game (Japan).rom",
            "Test Game (USA, Europe).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ];
        for romfile_name in &romfile_names {
            let romfile_path = tmp_directory.join(romfile_name);
            fs::copy(test_directory.join(romfile_name), &romfile_path)
                .await
                .unwrap();
            let matches = import_roms::subcommand()
                .get_matches_from(&["import-roms", &romfile_path.as_os_str().to_str().unwrap()]);
            import_roms::main(&mut connection, &matches, &progress_bar)
                .await
                .unwrap();
        }

        let system = find_systems(&mut connection).await.remove(0);
        let system_directory = get_system_directory(&mut connection, &progress_bar, &system)
            .await
            .unwrap();

        let matches = subcommand().get_matches_from(&["sort-roms", "-y"]);
        let all_regions = vec![];
        let one_regions = vec![];

        // when
        sort_system(
            &mut connection,
            &matches,
            &progress_bar,
            &system,
            &all_regions,
            &one_regions,
            &[],
            &[],
        )
        .await
        .unwrap();

        // then
        let romfiles = find_romfiles_by_system_id(&mut connection, system.id).await;
        assert_eq!(4, romfiles.len());

        let all_regions_indices = vec![0, 1, 2, 3];

        for i in all_regions_indices {
            let romfile = romfiles.get(i).unwrap();
            assert_eq!(
                &system_directory
                    .join(&romfile_names.get(i).unwrap())
                    .as_os_str()
                    .to_str()
                    .unwrap(),
                &romfile.path
            );
            assert_eq!(true, Path::new(&romfile.path).is_file().await);
        }
    }

    #[async_std::test]
    async fn test_sort_system_discard_beta() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));

        let matches = import_dats::subcommand()
            .get_matches_from(&["import-dats", "test/Test System (20200721).dat"]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let romfile_names = vec![
            "Test Game (Asia).rom",
            "Test Game (Japan).rom",
            "Test Game (USA, Europe).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ];
        for romfile_name in &romfile_names {
            let romfile_path = tmp_directory.join(romfile_name);
            fs::copy(test_directory.join(romfile_name), &romfile_path)
                .await
                .unwrap();
            let matches = import_roms::subcommand()
                .get_matches_from(&["import-roms", &romfile_path.as_os_str().to_str().unwrap()]);
            import_roms::main(&mut connection, &matches, &progress_bar)
                .await
                .unwrap();
        }

        let system = find_systems(&mut connection).await.remove(0);
        let system_directory = get_system_directory(&mut connection, &progress_bar, &system)
            .await
            .unwrap();

        let matches = subcommand().get_matches_from(&["config", "-y"]);
        let all_regions = vec![];
        let one_regions = vec![];

        // when
        sort_system(
            &mut connection,
            &matches,
            &progress_bar,
            &system,
            &all_regions,
            &one_regions,
            &["Beta"],
            &[],
        )
        .await
        .unwrap();

        // then
        let romfiles = find_romfiles_by_system_id(&mut connection, system.id).await;
        assert_eq!(4, romfiles.len());

        let all_regions_indices = vec![0, 1, 2];
        let trash_indices = vec![3];

        for i in all_regions_indices {
            let romfile = romfiles.get(i).unwrap();
            assert_eq!(
                &system_directory
                    .join(&romfile_names.get(i).unwrap())
                    .as_os_str()
                    .to_str()
                    .unwrap(),
                &romfile.path
            );
            assert_eq!(true, Path::new(&romfile.path).is_file().await);
        }

        for i in trash_indices {
            let romfile = romfiles.get(i).unwrap();
            assert_eq!(
                &system_directory
                    .join("Trash")
                    .join(&romfile_names.get(i).unwrap())
                    .as_os_str()
                    .to_str()
                    .unwrap(),
                &romfile.path
            );
            assert_eq!(true, Path::new(&romfile.path).is_file().await);
        }
    }

    #[async_std::test]
    async fn test_sort_system_discard_asia() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));

        let matches = import_dats::subcommand()
            .get_matches_from(&["import-dats", "test/Test System (20200721).dat"]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let romfile_names = vec![
            "Test Game (Asia).rom",
            "Test Game (Japan).rom",
            "Test Game (USA, Europe).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ];
        for romfile_name in &romfile_names {
            let romfile_path = tmp_directory.join(romfile_name);
            fs::copy(test_directory.join(romfile_name), &romfile_path)
                .await
                .unwrap();
            let matches = import_roms::subcommand()
                .get_matches_from(&["import-roms", &romfile_path.as_os_str().to_str().unwrap()]);
            import_roms::main(&mut connection, &matches, &progress_bar)
                .await
                .unwrap();
        }

        let system = find_systems(&mut connection).await.remove(0);
        let system_directory = get_system_directory(&mut connection, &progress_bar, &system)
            .await
            .unwrap();

        let matches = subcommand().get_matches_from(&["config", "-y"]);
        let all_regions = vec![Region::UnitedStates, Region::Europe, Region::Japan];
        let one_regions = vec![];

        // when
        sort_system(
            &mut connection,
            &matches,
            &progress_bar,
            &system,
            &all_regions,
            &one_regions,
            &[],
            &[],
        )
        .await
        .unwrap();

        // then
        let romfiles = find_romfiles_by_system_id(&mut connection, system.id).await;
        assert_eq!(4, romfiles.len());

        let all_regions_indices = vec![1, 2, 3];
        let trash_indices = vec![0];

        for i in all_regions_indices {
            let romfile = romfiles.get(i).unwrap();
            assert_eq!(
                &system_directory
                    .join(&romfile_names.get(i).unwrap())
                    .as_os_str()
                    .to_str()
                    .unwrap(),
                &romfile.path
            );
            assert_eq!(true, Path::new(&romfile.path).is_file().await);
        }

        for i in trash_indices {
            let romfile = romfiles.get(i).unwrap();
            assert_eq!(
                &system_directory
                    .join("Trash")
                    .join(&romfile_names.get(i).unwrap())
                    .as_os_str()
                    .to_str()
                    .unwrap(),
                &romfile.path
            );
            assert_eq!(true, Path::new(&romfile.path).is_file().await);
        }
    }

    #[async_std::test]
    async fn test_sort_system_discard_beta_and_asia() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));

        let matches = import_dats::subcommand()
            .get_matches_from(&["import-dats", "test/Test System (20200721).dat"]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let romfile_names = vec![
            "Test Game (Asia).rom",
            "Test Game (Japan).rom",
            "Test Game (USA, Europe).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ];
        for romfile_name in &romfile_names {
            let romfile_path = tmp_directory.join(romfile_name);
            fs::copy(test_directory.join(romfile_name), &romfile_path)
                .await
                .unwrap();
            let matches = import_roms::subcommand()
                .get_matches_from(&["import-roms", &romfile_path.as_os_str().to_str().unwrap()]);
            import_roms::main(&mut connection, &matches, &progress_bar)
                .await
                .unwrap();
        }

        let system = find_systems(&mut connection).await.remove(0);
        let system_directory = get_system_directory(&mut connection, &progress_bar, &system)
            .await
            .unwrap();

        let matches = subcommand().get_matches_from(&["config", "-y"]);
        let all_regions = vec![Region::UnitedStates, Region::Europe, Region::Japan];
        let one_regions = vec![];

        // when
        sort_system(
            &mut connection,
            &matches,
            &progress_bar,
            &system,
            &all_regions,
            &one_regions,
            &["Beta"],
            &[],
        )
        .await
        .unwrap();

        // then
        let romfiles = find_romfiles_by_system_id(&mut connection, system.id).await;
        assert_eq!(4, romfiles.len());

        let all_regions_indices = vec![1, 2];
        let trash_indices = vec![0, 3];

        for i in all_regions_indices {
            let romfile = romfiles.get(i).unwrap();
            assert_eq!(
                &system_directory
                    .join(&romfile_names.get(i).unwrap())
                    .as_os_str()
                    .to_str()
                    .unwrap(),
                &romfile.path
            );
            assert_eq!(true, Path::new(&romfile.path).is_file().await);
        }

        for i in trash_indices {
            let romfile = romfiles.get(i).unwrap();
            assert_eq!(
                &system_directory
                    .join("Trash")
                    .join(&romfile_names.get(i).unwrap())
                    .as_os_str()
                    .to_str()
                    .unwrap(),
                &romfile.path
            );
            assert_eq!(true, Path::new(&romfile.path).is_file().await);
        }
    }

    #[async_std::test]
    async fn test_sort_system_1g1r_with_parent_clone_information() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));

        let matches = import_dats::subcommand().get_matches_from(&[
            "import-dats",
            "test/Test System (20200721) (Parent-Clone).dat",
        ]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let romfile_names = vec![
            "Test Game (Asia).rom",
            "Test Game (Japan).rom",
            "Test Game (USA, Europe).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ];
        for romfile_name in &romfile_names {
            let romfile_path = tmp_directory.join(romfile_name);
            fs::copy(test_directory.join(romfile_name), &romfile_path)
                .await
                .unwrap();
            let matches = import_roms::subcommand()
                .get_matches_from(&["import-roms", &romfile_path.as_os_str().to_str().unwrap()]);
            import_roms::main(&mut connection, &matches, &progress_bar)
                .await
                .unwrap();
        }

        let system = find_systems(&mut connection).await.remove(0);
        let system_directory = get_system_directory(&mut connection, &progress_bar, &system)
            .await
            .unwrap();

        let matches = subcommand().get_matches_from(&["config", "-y"]);
        let all_regions = vec![];
        let one_regions = vec![Region::UnitedStates, Region::Europe];

        // when
        sort_system(
            &mut connection,
            &matches,
            &progress_bar,
            &system,
            &all_regions,
            &one_regions,
            &[],
            &[],
        )
        .await
        .unwrap();

        // then
        let romfiles = find_romfiles_by_system_id(&mut connection, system.id).await;
        assert_eq!(4, romfiles.len());

        let one_regions_indices = vec![2];
        let trash_indices = vec![0, 1, 3];

        for i in one_regions_indices {
            let romfile = romfiles.get(i).unwrap();
            assert_eq!(
                &system_directory
                    .join("1G1R")
                    .join(&romfile_names.get(i).unwrap())
                    .as_os_str()
                    .to_str()
                    .unwrap(),
                &romfile.path
            );
            assert_eq!(true, Path::new(&romfile.path).is_file().await);
        }

        for i in trash_indices {
            let romfile = romfiles.get(i).unwrap();
            assert_eq!(
                &system_directory
                    .join("Trash")
                    .join(&romfile_names.get(i).unwrap())
                    .as_os_str()
                    .to_str()
                    .unwrap(),
                &romfile.path
            );
            assert_eq!(true, Path::new(&romfile.path).is_file().await);
        }
    }

    #[async_std::test]
    async fn test_sort_system_1g1r_without_parent_clone_information() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));

        let matches = import_dats::subcommand()
            .get_matches_from(&["import-dats", "test/Test System (20200721).dat"]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let romfile_names = vec![
            "Test Game (Asia).rom",
            "Test Game (Japan).rom",
            "Test Game (USA, Europe).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ];
        for romfile_name in &romfile_names {
            let romfile_path = tmp_directory.join(romfile_name);
            fs::copy(test_directory.join(romfile_name), &romfile_path)
                .await
                .unwrap();
            let matches = import_roms::subcommand()
                .get_matches_from(&["import-roms", &romfile_path.as_os_str().to_str().unwrap()]);
            import_roms::main(&mut connection, &matches, &progress_bar)
                .await
                .unwrap();
        }

        let system = find_systems(&mut connection).await.remove(0);
        let system_directory = get_system_directory(&mut connection, &progress_bar, &system)
            .await
            .unwrap();

        let matches = subcommand().get_matches_from(&["config", "-y"]);
        let all_regions = vec![];
        let one_regions = vec![Region::UnitedStates, Region::Europe];

        // when
        sort_system(
            &mut connection,
            &matches,
            &progress_bar,
            &system,
            &all_regions,
            &one_regions,
            &[],
            &[],
        )
        .await
        .unwrap();

        // then
        let romfiles = find_romfiles_by_system_id(&mut connection, system.id).await;
        assert_eq!(4, romfiles.len());

        let one_regions_indices = vec![2, 3];
        let trash_indices = vec![0, 1];

        for i in one_regions_indices {
            let romfile = romfiles.get(i).unwrap();
            assert_eq!(
                &system_directory
                    .join("1G1R")
                    .join(&romfile_names.get(i).unwrap())
                    .as_os_str()
                    .to_str()
                    .unwrap(),
                &romfile.path
            );
            assert_eq!(true, Path::new(&romfile.path).is_file().await);
        }

        for i in trash_indices {
            let romfile = romfiles.get(i).unwrap();
            assert_eq!(
                &system_directory
                    .join("Trash")
                    .join(&romfile_names.get(i).unwrap())
                    .as_os_str()
                    .to_str()
                    .unwrap(),
                &romfile.path
            );
            assert_eq!(true, Path::new(&romfile.path).is_file().await);
        }
    }

    #[async_std::test]
    async fn test_sort_system_1g1r_with_parent_clone_information_without_asia_without_beta() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));

        let matches = import_dats::subcommand().get_matches_from(&[
            "import-dats",
            "test/Test System (20200721) (Parent-Clone).dat",
        ]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let romfile_names = vec![
            "Test Game (Asia).rom",
            "Test Game (Japan).rom",
            "Test Game (USA, Europe).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ];
        for romfile_name in &romfile_names {
            let romfile_path = tmp_directory.join(romfile_name);
            fs::copy(test_directory.join(romfile_name), &romfile_path)
                .await
                .unwrap();
            let matches = import_roms::subcommand()
                .get_matches_from(&["import-roms", &romfile_path.as_os_str().to_str().unwrap()]);
            import_roms::main(&mut connection, &matches, &progress_bar)
                .await
                .unwrap();
        }

        let system = find_systems(&mut connection).await.remove(0);
        let system_directory = get_system_directory(&mut connection, &progress_bar, &system)
            .await
            .unwrap();

        let matches = subcommand().get_matches_from(&["config", "-y"]);
        let all_regions = vec![Region::Japan];
        let one_regions = vec![Region::UnitedStates, Region::Europe];

        // when
        sort_system(
            &mut connection,
            &matches,
            &progress_bar,
            &system,
            &all_regions,
            &one_regions,
            &["Beta"],
            &[],
        )
        .await
        .unwrap();

        // then
        let romfiles = find_romfiles_by_system_id(&mut connection, system.id).await;
        assert_eq!(4, romfiles.len());

        let all_regions_indices = vec![1];
        let one_regions_indices = vec![2];
        let trash_indices = vec![0, 3];

        for i in all_regions_indices {
            let romfile = romfiles.get(i).unwrap();
            assert_eq!(
                &system_directory
                    .join(&romfile_names.get(i).unwrap())
                    .as_os_str()
                    .to_str()
                    .unwrap(),
                &romfile.path
            );
            assert_eq!(true, Path::new(&romfile.path).is_file().await);
        }

        for i in one_regions_indices {
            let romfile = romfiles.get(i).unwrap();
            assert_eq!(
                &system_directory
                    .join("1G1R")
                    .join(&romfile_names.get(i).unwrap())
                    .as_os_str()
                    .to_str()
                    .unwrap(),
                &romfile.path
            );
            assert_eq!(true, Path::new(&romfile.path).is_file().await);
        }

        for i in trash_indices {
            let romfile = romfiles.get(i).unwrap();
            assert_eq!(
                &system_directory
                    .join("Trash")
                    .join(&romfile_names.get(i).unwrap())
                    .as_os_str()
                    .to_str()
                    .unwrap(),
                &romfile.path
            );
            assert_eq!(true, Path::new(&romfile.path).is_file().await);
        }
    }
}

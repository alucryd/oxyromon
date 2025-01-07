use super::common::*;
use super::config::*;
use super::database::*;
use super::generate_playlists::DISC_REGEX;
use super::mimetype::*;
use super::model::*;
use super::prompt::*;
use super::util::*;
use super::SimpleResult;
use clap::builder::PossibleValuesParser;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use itertools::Itertools;
use rayon::prelude::*;
use regex::Regex;
use shiratsu_naming::naming::nointro::{NoIntroName, NoIntroToken};
use shiratsu_naming::naming::TokenizedName;
use shiratsu_naming::region::Region;
use sqlx::sqlite::SqliteConnection;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;
use strum::VariantNames;

lazy_static! {
    pub static ref LANGUAGE_REGEX: Regex = Regex::new(r"[A-Z][a-z]").unwrap();
    pub static ref VARIANT_REGEX: Regex = Regex::new(r"[A-Z]{2}").unwrap();
}

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
            Arg::new("REGIONS_ALL_SUBFOLDERS")
                .long("subfolders")
                .help("Set the subfolders scheme for games")
                .required(false)
                .num_args(1)
                .value_parser(PossibleValuesParser::new(SubfolderScheme::VARIANTS)),
        )
        .arg(
            Arg::new("REGIONS_ONE")
                .short('o')
                .long("1g1r")
                .help("Set the 1G1R regions to keep (ordered)")
                .required(false)
                .num_args(1..),
        )
        .arg(
            Arg::new("REGIONS_ONE_SUBFOLDERS")
                .long("1g1r-subfolders")
                .help("Set the subfolders scheme for 1G1R games")
                .required(false)
                .num_args(1)
                .value_parser(PossibleValuesParser::new(SubfolderScheme::VARIANTS)),
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

    let all_regions = get_regions(connection, matches, "REGIONS_ALL").await;
    let one_regions = get_regions(connection, matches, "REGIONS_ONE").await;
    let languages = get_list(connection, "LANGUAGES").await;
    let ignored_releases = get_list(connection, "DISCARD_RELEASES").await;
    let ignored_flags = get_list(connection, "DISCARD_FLAGS").await;
    let prefer_parents = get_bool(connection, "PREFER_PARENTS").await;
    let preferred_regions =
        PreferredRegion::from_str(&get_string(connection, "PREFER_REGIONS").await.unwrap())
            .unwrap();
    let preferred_versions =
        PreferredVersion::from_str(&get_string(connection, "PREFER_VERSIONS").await.unwrap())
            .unwrap();
    let preferred_flags = get_list(connection, "PREFER_FLAGS").await;
    let all_regions_subfolders = SubfolderScheme::from_str(
        matches
            .get_one::<String>("REGIONS_ALL_SUBFOLDERS")
            .unwrap_or(
                &get_string(connection, "REGIONS_ALL_SUBFOLDERS")
                    .await
                    .unwrap(),
            ),
    )
    .unwrap();
    let one_regions_subfolders = SubfolderScheme::from_str(
        matches
            .get_one::<String>("REGIONS_ONE_SUBFOLDERS")
            .unwrap_or(
                &get_string(connection, "REGIONS_ONE_SUBFOLDERS")
                    .await
                    .unwrap(),
            ),
    )
    .unwrap();
    let one_regions_strict = get_bool(connection, "REGIONS_ONE_STRICT").await;

    let answer_yes = matches.get_flag("YES");
    let print_wanted = matches.get_flag("WANTED");

    for system in systems {
        sort_system(
            connection,
            progress_bar,
            answer_yes,
            print_wanted,
            &system,
            &all_regions,
            &one_regions,
            &languages.iter().map(String::as_str).collect_vec(),
            &ignored_releases.iter().map(String::as_str).collect_vec(),
            &ignored_flags.iter().map(String::as_str).collect_vec(),
            prefer_parents,
            &preferred_regions,
            &preferred_versions,
            &preferred_flags.iter().map(String::as_str).collect_vec(),
            &all_regions_subfolders,
            &one_regions_subfolders,
            one_regions_strict,
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

#[allow(clippy::too_many_arguments)]
async fn sort_system(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    answer_yes: bool,
    print_wanted: bool,
    system: &System,
    all_regions: &[Region],
    one_regions: &[Region],
    languages: &[&str],
    ignored_releases: &[&str],
    ignored_flags: &[&str],
    prefer_parents: bool,
    preferred_regions: &PreferredRegion,
    preferred_versions: &PreferredVersion,
    preferred_flags: &[&str],
    all_regions_subfolders: &SubfolderScheme,
    one_regions_subfolders: &SubfolderScheme,
    one_regions_strict: bool,
) -> SimpleResult<()> {
    progress_bar.enable_steady_tick(Duration::from_millis(100));
    progress_bar.println(format!("Processing \"{}\"", system.name));

    let mut games: Vec<Game>;
    let mut all_regions_games: Vec<Game> = vec![];
    let mut one_region_games: Vec<Game> = vec![];
    let mut ignored_games: Vec<Game> = vec![];
    let mut incomplete_all_regions_games: Vec<Game> = vec![];
    let mut incomplete_one_region_games: Vec<Game> = vec![];
    let mut romfile_moves: Vec<(&Romfile, PathBuf)> = vec![];

    let romfiles = find_romfiles_by_system_id(connection, system.id).await;
    let mut romfiles_by_id: HashMap<i64, Romfile> = romfiles
        .into_iter()
        .map(|romfile| (romfile.id, romfile))
        .collect();

    let patch_romfiles = find_patch_romfiles_by_system_id(connection, system.id).await;
    for romfile in patch_romfiles {
        romfiles_by_id.insert(romfile.id, romfile);
    }

    let playlist_romfiles = find_playlist_romfiles_by_system_id(connection, system.id).await;
    for romfile in playlist_romfiles {
        romfiles_by_id.insert(romfile.id, romfile);
    }

    // 1G1R mode
    if !system.arcade && !one_regions.is_empty() {
        let parent_games = find_parent_games_by_system_id(connection, system.id).await;
        let clone_games = find_clone_games_by_system_id(connection, system.id).await;

        let mut clone_games_by_parent_id: HashMap<i64, Vec<Game>> = HashMap::new();
        clone_games.into_iter().for_each(|game| {
            let group = clone_games_by_parent_id
                .entry(game.parent_id.unwrap())
                .or_default();
            group.push(game);
        });

        for parent_game in parent_games {
            if clone_games_by_parent_id.contains_key(&parent_game.id) {
                games = clone_games_by_parent_id.remove(&parent_game.id).unwrap();
            } else {
                games = vec![];
            }
            games.push(parent_game);
            // put newer releases first
            games.sort_by(|a, b| {
                sort_games_by_weight(
                    a,
                    b,
                    prefer_parents,
                    preferred_regions,
                    preferred_versions,
                    preferred_flags,
                )
            });

            // trim ignored games
            if !ignored_releases.is_empty() || !ignored_flags.is_empty() {
                let (mut left_games, right_games) = trim_ignored_games(
                    games,
                    languages,
                    ignored_releases,
                    ignored_flags,
                    system.arcade,
                );
                ignored_games.append(&mut left_games);
                games = right_games;
            }

            // find the one game we want to keep, if any
            for region in one_regions {
                let i = games.iter().position(|game| {
                    (game.completion == Completion::Full as i64
                        || one_regions_strict
                        || games
                            .iter()
                            .all(|game| game.completion != Completion::Full as i64))
                        && Region::try_from_tosec_region(&game.regions)
                            .unwrap_or_default()
                            .contains(region)
                });
                if let Some(i) = i {
                    let game = games.remove(i);
                    if game.completion == Completion::Full as i64 {
                        one_region_games.push(game);
                    } else {
                        incomplete_one_region_games.push(game);
                    }
                    break;
                }
            }

            // go through the remaining games
            while !games.is_empty() {
                let game = games.remove(0);
                let region_in_all_regions = all_regions.contains(&Region::Unknown)
                    || all_regions.iter().any(|region| {
                        Region::try_from_tosec_region(&game.regions)
                            .unwrap_or_default()
                            .contains(region)
                    });
                if region_in_all_regions {
                    if game.completion == Completion::Full as i64 {
                        all_regions_games.push(game);
                    } else {
                        incomplete_all_regions_games.push(game);
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
            let (mut left_games, right_games) = trim_ignored_games(
                games,
                languages,
                ignored_releases,
                ignored_flags,
                system.arcade,
            );
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
                if game.completion == Completion::Full as i64 {
                    all_regions_games.push(game);
                } else {
                    incomplete_all_regions_games.push(game);
                }
            } else {
                ignored_games.push(game);
            }
        }
    } else {
        games = find_games_by_system_id(connection, system.id).await;

        // trim ignored games
        if !ignored_releases.is_empty() || !ignored_flags.is_empty() {
            let (mut left_games, right_games) = trim_ignored_games(
                games,
                languages,
                ignored_releases,
                ignored_flags,
                system.arcade,
            );
            ignored_games.append(&mut left_games);
            games = right_games;
        }

        for game in games {
            if game.completion == Completion::Full as i64 {
                all_regions_games.push(game);
            } else {
                incomplete_all_regions_games.push(game)
            }
        }
    }

    if print_wanted {
        let mut all_incomplete_games: Vec<&Game> = vec![];
        all_incomplete_games.extend(incomplete_all_regions_games.iter());
        all_incomplete_games.extend(incomplete_one_region_games.iter());
        all_incomplete_games.sort_by_key(|game| &game.name);
        let mut wanted_roms: Vec<Rom> = find_roms_without_romfile_by_game_ids(
            connection,
            &all_incomplete_games
                .par_iter()
                .map(|game| game.id)
                .collect::<Vec<i64>>(),
        )
        .await;

        if !wanted_roms.is_empty() {
            progress_bar.println("Wanted:");
            wanted_roms.sort_by_key(|rom| rom.game_id);
            for rom in wanted_roms {
                let game = all_incomplete_games
                    .iter()
                    .find(|&game| game.id == rom.game_id)
                    .unwrap();
                progress_bar.println(format!(
                    "{} ({}) [{}]",
                    rom.name,
                    game.name,
                    rom.crc.as_ref().unwrap_or(rom.sha1.as_ref().unwrap())
                ));
            }
        } else {
            progress_bar.println("No wanted ROMs");
        }
    }

    let system_directory = get_system_directory(connection, system).await?;
    let one_region_directory = get_one_region_directory(connection, system).await?;
    let trash_directory = get_trash_directory(connection, Some(system)).await?;

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
            all_regions_subfolders,
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
            one_regions_subfolders,
        )
        .await?,
    );

    // process incomplete games
    changes += update_games_sorting(
        &mut transaction,
        &incomplete_one_region_games
            .iter()
            .map(|game| game.id)
            .collect::<Vec<i64>>(),
        Sorting::OneRegion,
    )
    .await;
    changes += update_games_sorting(
        &mut transaction,
        &incomplete_all_regions_games
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
            incomplete_one_region_games,
            &system_directory,
            &romfiles_by_id,
            one_regions_subfolders,
        )
        .await?,
    );
    romfile_moves.append(
        &mut sort_games(
            &mut transaction,
            system,
            incomplete_all_regions_games,
            &system_directory,
            &romfiles_by_id,
            all_regions_subfolders,
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
            &SubfolderScheme::None,
        )
        .await?,
    );

    progress_bar.disable_steady_tick();

    if !romfile_moves.is_empty() {
        // sort moves and print a summary
        romfile_moves.sort_by(|a, b| a.1.cmp(&b.1));
        romfile_moves.dedup_by(|a, b| a.1 == b.1);

        progress_bar.println("Summary:");
        for romfile_move in &romfile_moves {
            progress_bar.println(format!(
                "\"{}\" -> \"{}\"",
                Path::new(&romfile_move.0.path)
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap(),
                romfile_move.1.as_os_str().to_str().unwrap()
            ));
        }

        // prompt user for confirmation
        if answer_yes || confirm(true)? {
            for romfile_move in romfile_moves {
                romfile_move
                    .0
                    .as_common(&mut transaction)
                    .await?
                    .rename(progress_bar, &romfile_move.1, true)
                    .await?
                    .update(&mut transaction, progress_bar, romfile_move.0.id)
                    .await?;
                // delete empty directories
                let mut directory = romfile_move
                    .0
                    .as_common(&mut transaction)
                    .await?
                    .path
                    .parent()
                    .unwrap()
                    .to_path_buf();
                while directory.read_dir().unwrap().next().is_none() {
                    if directory == system_directory {
                        break;
                    } else {
                        remove_directory(progress_bar, &directory, true).await?;
                        directory = directory.parent().unwrap().to_path_buf();
                    }
                }
            }
            commit_transaction(transaction).await;
        } else {
            rollback_transaction(transaction).await;
        }
    } else {
        commit_transaction(transaction).await;
        progress_bar.println("Nothing to do");
    }

    // update games and systems completion
    if changes > 0 {
        compute_system_completion(connection, progress_bar, system).await;
    }

    Ok(())
}

async fn sort_games<'a, P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    system: &System,
    games: Vec<Game>,
    destination_directory: &P,
    romfiles_by_id: &'a HashMap<i64, Romfile>,
    subfolders: &SubfolderScheme,
) -> SimpleResult<Vec<(&'a Romfile, PathBuf)>> {
    let mut romfile_moves: Vec<(&Romfile, PathBuf)> = vec![];

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
        let group = roms_by_game_id.entry(rom.game_id).or_default();
        group.push(rom);
    });

    for game in games {
        let roms = roms_by_game_id.get(&game.id);
        let roms = match roms {
            Some(roms) => roms,
            None => continue,
        };
        for rom in roms {
            let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
            let extension = Path::new(&romfile.path)
                .extension()
                .map(|extension| extension.to_str().unwrap());
            let new_romfile_path = compute_new_romfile_path(
                system,
                &game,
                rom,
                extension,
                destination_directory,
                subfolders,
            )
            .await?;
            if romfile.as_common(connection).await?.path != new_romfile_path {
                let patches = find_patches_by_rom_id(connection, rom.id).await;
                for patch in patches {
                    let patch_romfile = romfiles_by_id.get(&patch.romfile_id).unwrap();
                    let patch_extension = Path::new(&romfile.path)
                        .extension()
                        .unwrap()
                        .to_str()
                        .unwrap();
                    let new_patch_romfile_path = new_romfile_path.with_extension(patch_extension);
                    romfile_moves.push((patch_romfile, new_patch_romfile_path));
                }
                romfile_moves.push((romfile, new_romfile_path));
            }
        }
        if game.playlist_id.is_some() {
            let playlist_romfile = romfiles_by_id.get(&game.playlist_id.unwrap()).unwrap();
            let new_playlist_romfile_path =
                compute_new_playlist_path(&game, destination_directory, subfolders).await?;
            if playlist_romfile.as_common(connection).await?.path != new_playlist_romfile_path {
                romfile_moves.push((playlist_romfile, new_playlist_romfile_path));
            }
        }
    }

    Ok(romfile_moves)
}

fn trim_ignored_games(
    games: Vec<Game>,
    languages: &[&str],
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
            log::debug!("sort_roms::trim_ignored_games(\"{}\")", &game.name);
            if let Ok(name) = NoIntroName::try_parse(&game.name) {
                for token in name.iter() {
                    if let NoIntroToken::Languages(parsed_languages) = token {
                        log::debug!("parsed languages: {:?}", parsed_languages);
                        if !languages.is_empty() {
                            let clean_parsed_languages = parsed_languages
                                .iter()
                                .filter(|(language, variant)| {
                                    LANGUAGE_REGEX.is_match(language)
                                        && (variant.is_none()
                                            || VARIANT_REGEX.is_match(variant.unwrap()))
                                })
                                .map(|(language, _)| language.to_string())
                                .collect::<Vec<String>>();
                            if !clean_parsed_languages.is_empty()
                                && !clean_parsed_languages
                                    .iter()
                                    .any(|language| languages.contains(&language.as_str()))
                            {
                                return true;
                            }
                        }
                    }
                    if let NoIntroToken::Release(release, _) = token {
                        log::debug!("release: {}", release);
                        if ignored_releases.contains(release) {
                            log::debug!("ignoring release");
                            return true;
                        }
                    }
                    if let NoIntroToken::Flag(_, flags) = token {
                        log::debug!("flags: {}", flags);
                        if ignored_flags.contains(flags) {
                            log::debug!("ignoring flag: {}", flags);
                            return true;
                        }
                        for flag in flags.split(", ") {
                            if ignored_flags.contains(&flag) {
                                log::debug!("ignoring flag: {}", flag);
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

fn sort_games_by_weight(
    game_a: &Game,
    game_b: &Game,
    prefer_parents: bool,
    preferred_regions: &PreferredRegion,
    preferred_versions: &PreferredVersion,
    preferred_flags: &[&str],
) -> Ordering {
    let mut weight_a: u8 = 0;
    let mut weight_b: u8 = 0;

    if prefer_parents {
        if game_a.parent_id.is_none() {
            weight_a += 1;
        } else if game_b.parent_id.is_none() {
            weight_b += 1;
        }
    }

    if preferred_regions != &PreferredRegion::None {
        let regions_a = Region::try_from_tosec_region(&game_a.regions).unwrap_or_default();
        let regions_b = Region::try_from_tosec_region(&game_b.regions).unwrap_or_default();

        match regions_b.len().partial_cmp(&regions_a.len()).unwrap() {
            Ordering::Less => match preferred_regions {
                PreferredRegion::Broad => weight_a += 1,
                PreferredRegion::Narrow => weight_b += 1,
                PreferredRegion::None => {}
            },
            Ordering::Greater => match preferred_regions {
                PreferredRegion::Broad => weight_b += 1,
                PreferredRegion::Narrow => weight_a += 1,
                PreferredRegion::None => {}
            },
            Ordering::Equal => {}
        };
    }

    if preferred_versions != &PreferredVersion::None {
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
            match version_b.partial_cmp(version_a).unwrap() {
                Ordering::Less => match *preferred_versions {
                    PreferredVersion::New => weight_a += 1,
                    PreferredVersion::Old => weight_b += 1,
                    PreferredVersion::None => {}
                },
                Ordering::Greater => match *preferred_versions {
                    PreferredVersion::New => weight_b += 1,
                    PreferredVersion::Old => weight_a += 1,
                    PreferredVersion::None => {}
                },
                Ordering::Equal => {}
            };
        } else if version_a.is_some() {
            match *preferred_versions {
                PreferredVersion::New => weight_a += 1,
                PreferredVersion::Old => weight_b += 1,
                PreferredVersion::None => {}
            }
        } else if version_b.is_some() {
            match *preferred_versions {
                PreferredVersion::New => weight_b += 1,
                PreferredVersion::Old => weight_a += 1,
                PreferredVersion::None => {}
            }
        }
    }

    if !preferred_flags.is_empty() {
        if let Ok(name) = NoIntroName::try_parse(&game_a.name) {
            for token in name.iter() {
                if let NoIntroToken::Flag(_, flags) = token {
                    for flag in flags.split(", ") {
                        if preferred_flags.contains(&flag) {
                            weight_a += 1;
                        }
                    }
                }
            }
        }
        if let Ok(name) = NoIntroName::try_parse(&game_b.name) {
            for token in name.iter() {
                if let NoIntroToken::Flag(_, flags) = token {
                    for flag in flags.split(", ") {
                        if preferred_flags.contains(&flag) {
                            weight_b += 1;
                        }
                    }
                }
            }
        }
    }

    // compare in reverse, we want higher weight first
    weight_b.partial_cmp(&weight_a).unwrap()
}

async fn compute_new_romfile_path<P: AsRef<Path>>(
    system: &System,
    game: &Game,
    rom: &Rom,
    extension: Option<&str>,
    destination_directory: &P,
    subfolders: &SubfolderScheme,
) -> SimpleResult<PathBuf> {
    let mut non_original_extensions = vec![
        CHD_EXTENSION,
        CSO_EXTENSION,
        NSZ_EXTENSION,
        RVZ_EXTENSION,
        ZSO_EXTENSION,
    ];
    non_original_extensions.append(&mut ARCHIVE_EXTENSIONS.to_vec());

    let mut new_romfile_path: PathBuf = destination_directory.as_ref().to_path_buf();

    // subfolders
    if subfolders == &SubfolderScheme::Alpha {
        if extension.is_some() && non_original_extensions.contains(extension.as_ref().unwrap())
            || system.arcade
            || game.jbfolder
        {
            new_romfile_path = new_romfile_path.join(compute_alpha_subfolder(&game.name));
        } else {
            new_romfile_path = new_romfile_path.join(compute_alpha_subfolder(&rom.name));
        }
    }

    // arcade and jbfolder in subdirectories unless they are archives
    if system.arcade
        && (extension.is_none()
            || extension.is_some() && !ARCHIVE_EXTENSIONS.contains(extension.as_ref().unwrap()))
        || game.jbfolder
    {
        new_romfile_path = new_romfile_path.join(&game.name);
    }

    // file name
    if extension.is_some() && non_original_extensions.contains(extension.as_ref().unwrap()) {
        if system.arcade && !ARCHIVE_EXTENSIONS.contains(extension.as_ref().unwrap()) {
            new_romfile_path =
                new_romfile_path.join(format!("{}.{}", &rom.name, extension.as_ref().unwrap()));
        } else {
            new_romfile_path =
                new_romfile_path.join(format!("{}.{}", &game.name, extension.as_ref().unwrap()));
        }
    } else {
        new_romfile_path = new_romfile_path.join(&rom.name);
    }

    Ok(new_romfile_path)
}

async fn compute_new_playlist_path<P: AsRef<Path>>(
    game: &Game,
    destination_directory: &P,
    subfolders: &SubfolderScheme,
) -> SimpleResult<PathBuf> {
    let mut new_playlist_path: PathBuf = destination_directory.as_ref().to_path_buf();
    if subfolders == &SubfolderScheme::Alpha {
        new_playlist_path = new_playlist_path.join(compute_alpha_subfolder(&game.name));
    }
    new_playlist_path = new_playlist_path.join(format!(
        "{}.{}",
        DISC_REGEX.replace(&game.name, ""),
        M3U_EXTENSION
    ));
    Ok(new_playlist_path)
}

fn compute_alpha_subfolder(name: &str) -> String {
    let first_char = name.chars().next().unwrap();
    if first_char.is_ascii_alphabetic() {
        first_char.to_ascii_uppercase().to_string()
    } else {
        String::from("#")
    }
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
mod test_order_no_prefer_parents_clone_vs_clone;
#[cfg(test)]
mod test_order_no_prefer_parents_clone_vs_parent;
#[cfg(test)]
mod test_order_no_prefer_parents_parent_vs_clone;
#[cfg(test)]
mod test_order_prefer_flags;
#[cfg(test)]
mod test_order_prefer_flags_swapped;
#[cfg(test)]
mod test_order_prefer_parents_clone_vs_clone;
#[cfg(test)]
mod test_order_prefer_parents_clone_vs_parent;
#[cfg(test)]
mod test_order_prefer_parents_parent_vs_clone;
#[cfg(test)]
mod test_order_prefer_regions_broad;
#[cfg(test)]
mod test_order_prefer_regions_broad_swapped;
#[cfg(test)]
mod test_order_prefer_regions_narrow;
#[cfg(test)]
mod test_order_prefer_regions_narrow_swapped;
#[cfg(test)]
mod test_order_prefer_versions_new_revision_vs_revision;
#[cfg(test)]
mod test_order_prefer_versions_new_revision_vs_revision_swapped;
#[cfg(test)]
mod test_order_prefer_versions_new_revision_vs_vanilla;
#[cfg(test)]
mod test_order_prefer_versions_new_vanilla_vs_revision;
#[cfg(test)]
mod test_order_prefer_versions_old_revision_vs_revision;
#[cfg(test)]
mod test_order_prefer_versions_old_revision_vs_revision_swapped;
#[cfg(test)]
mod test_order_prefer_versions_old_revision_vs_vanilla;
#[cfg(test)]
mod test_order_prefer_versions_old_vanilla_vs_revision;
#[cfg(test)]
mod test_path_archive_multiple_files;
#[cfg(test)]
mod test_path_archive_single_file;
#[cfg(test)]
mod test_path_chd_multiple_tracks;
#[cfg(test)]
mod test_path_chd_single_track;
#[cfg(test)]
mod test_path_cso;
#[cfg(test)]
mod test_path_original;
#[cfg(test)]
mod test_path_playlist;
#[cfg(test)]
mod test_path_playlist_subfolder_alpha;
#[cfg(test)]
mod test_path_rvz;
#[cfg(test)]
mod test_path_subfolder_alpha_letter;
#[cfg(test)]
mod test_path_subfolder_alpha_other;
#[cfg(test)]
mod test_sort;
#[cfg(test)]
mod test_sort_1g1r;
#[cfg(test)]
mod test_sort_1g1r_catch_all;
#[cfg(test)]
mod test_sort_1g1r_discard_asia_and_beta;
#[cfg(test)]
mod test_sort_1g1r_lenient;
#[cfg(test)]
mod test_sort_1g1r_playlist;
#[cfg(test)]
mod test_sort_1g1r_revisions;
#[cfg(test)]
mod test_sort_1g1r_strict;
#[cfg(test)]
mod test_sort_1g1r_subfolders_alpha;
#[cfg(test)]
mod test_sort_1g1r_without_parent_clone;
#[cfg(test)]
mod test_sort_1g1r_without_roms;
#[cfg(test)]
mod test_sort_discard_asia;
#[cfg(test)]
mod test_sort_discard_asia_and_beta;
#[cfg(test)]
mod test_sort_discard_beta;
#[cfg(test)]
mod test_trim_ignored;

use super::chdman::*;
use super::config::*;
use super::database::*;
use super::maxcso::*;
use super::model::*;
use super::progress::*;
use super::prompt::*;
use super::sevenzip::*;
use super::util::*;
use super::SimpleResult;
use async_std::path::{Path, PathBuf};
use clap::{App, Arg, ArgMatches, SubCommand};
use rayon::prelude::*;
use regex::Regex;
use sqlx::SqliteConnection;
use std::collections::HashMap;
use std::ffi::OsString;

pub fn subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name("sort-roms")
        .about("Sorts ROM files according to region and version preferences")
        .arg(
            Arg::with_name("REGIONS")
                .short("r")
                .long("regions")
                .help("Sets the regions to keep (unordered)")
                .required(false)
                .takes_value(true)
                .multiple(true),
        )
        .arg(
            Arg::with_name("1G1R")
                .short("g")
                .long("1g1r")
                .help("Sets the 1G1R regions to keep (ordered)")
                .required(false)
                .takes_value(true)
                .multiple(true),
        )
        .arg(
            Arg::with_name("WITH_BETA")
                .long("with-beta")
                .help("Keeps beta games")
                .required(false),
        )
        .arg(
            Arg::with_name("WITHOUT_BETA")
                .long("without-beta")
                .help("Discards beta games")
                .required(false)
                .conflicts_with("WITH_BETA"),
        )
        .arg(
            Arg::with_name("WITH_DEBUG")
                .long("with-debug")
                .help("Keeps debug games")
                .required(false),
        )
        .arg(
            Arg::with_name("WITHOUT_DEBUG")
                .long("without-debug")
                .help("Discards debug games")
                .required(false)
                .conflicts_with("WITH_DEBUG"),
        )
        .arg(
            Arg::with_name("WITH_DEMO")
                .long("with-demo")
                .help("Keeps demo games")
                .required(false),
        )
        .arg(
            Arg::with_name("WITHOUT_DEMO")
                .long("without-demo")
                .help("Discards demo games")
                .required(false)
                .conflicts_with("WITH_DEMO"),
        )
        .arg(
            Arg::with_name("WITH_PROGRAM")
                .long("with-program")
                .help("Keeps program games")
                .required(false),
        )
        .arg(
            Arg::with_name("WITHOUT_PROGRAM")
                .long("without-program")
                .help("Discards program games")
                .required(false)
                .conflicts_with("WITH_PROGRAM"),
        )
        .arg(
            Arg::with_name("WITH_PROTO")
                .long("with-proto")
                .help("Keeps prototype games")
                .required(false),
        )
        .arg(
            Arg::with_name("WITHOUT_PROTO")
                .long("without-proto")
                .help("Discards prototype games")
                .required(false)
                .conflicts_with("WITH_PROTO"),
        )
        .arg(
            Arg::with_name("WITH_SAMPLE")
                .long("with-sample")
                .help("Keeps sample games")
                .required(false),
        )
        .arg(
            Arg::with_name("WITHOUT_SAMPLE")
                .long("without-sample")
                .help("Discards sample games")
                .required(false)
                .conflicts_with("WITH_SAMPLE"),
        )
        .arg(
            Arg::with_name("WITH_SEGA_CHANNEL")
                .long("with-sega-channel")
                .help("Keeps sega channel games")
                .required(false),
        )
        .arg(
            Arg::with_name("WITHOUT_SEGA_CHANNEL")
                .long("without-sega-channel")
                .help("Discards sega channel games")
                .required(false)
                .conflicts_with("WITH_SEGA_CHANNEL"),
        )
        .arg(
            Arg::with_name("WITH_VIRTUAL_CONSOLE")
                .long("with-virtual-console")
                .help("Keeps virtual console games")
                .required(false),
        )
        .arg(
            Arg::with_name("WITHOUT_VIRTUAL_CONSOLE")
                .long("without-virtual-console")
                .help("Discards virtual console games")
                .required(false)
                .conflicts_with("WITH_VIRTUAL_CONSOLE"),
        )
        .arg(
            Arg::with_name("MISSING")
                .short("m")
                .long("missing")
                .help("Shows missing games")
                .required(false),
        )
        .arg(
            Arg::with_name("ALL")
                .short("a")
                .long("all")
                .help("Sorts all systems")
                .required(false),
        )
        .arg(
            Arg::with_name("YES")
                .short("y")
                .long("yes")
                .help("Automatically says yes to prompts")
                .required(false),
        )
}

pub async fn main(connection: &mut SqliteConnection, matches: &ArgMatches<'_>) -> SimpleResult<()> {
    let progress_bar = get_progress_bar(0, get_none_progress_style());

    let systems = prompt_for_systems(connection, matches.is_present("ALL")).await;

    // unordered regions to keep
    let mut all_regions: Vec<&str> = Vec::new();
    if matches.is_present("REGIONS") {
        all_regions = matches.values_of("REGIONS").unwrap().collect();
    }

    // ordered regions to use for 1G1R
    let mut one_regions: Vec<&str> = Vec::new();
    if matches.is_present("1G1R") {
        one_regions = matches.values_of("1G1R").unwrap().collect();
    }

    // unwanted regex
    let unwanted_regex = compute_unwanted_regex(connection, matches).await;

    for system in systems {
        progress_bar.println(&format!("Processing {}", system.name));
        progress_bar.set_message("Processing games");

        let mut games: Vec<Game>;
        let mut all_regions_games: Vec<Game> = Vec::new();
        let mut one_region_games: Vec<Game> = Vec::new();
        let mut trash_games: Vec<Game> = Vec::new();
        let mut romfile_moves: Vec<(&Romfile, String)> = Vec::new();

        let romfiles = find_romfiles_by_system_id(connection, system.id).await;
        let romfiles_by_id: HashMap<i64, Romfile> = romfiles
            .into_iter()
            .map(|romfile| (romfile.id, romfile))
            .collect();

        // 1G1R mode
        if !one_regions.is_empty() {
            let parent_games = find_parent_games_by_system_id(connection, system.id).await;
            let clone_games = find_clone_games_by_system_id(connection, system.id).await;

            let mut clone_games_by_parent_id: HashMap<i64, Vec<Game>> = HashMap::new();
            clone_games.into_iter().for_each(|game| {
                let group = clone_games_by_parent_id
                    .entry(game.parent_id.unwrap())
                    .or_insert(vec![]);
                group.push(game);
            });

            for parent in parent_games {
                games = clone_games_by_parent_id.remove(&parent.id).unwrap();
                games.insert(0, parent);

                // trim unwanted games
                match unwanted_regex.as_ref() {
                    Some(unwanted_regex) => {
                        let (mut unwanted_games, regular_games): (Vec<Game>, Vec<Game>) = games
                            .into_par_iter()
                            .partition(|game| unwanted_regex.find(&game.name).is_some());
                        trash_games.append(&mut unwanted_games);
                        games = regular_games;
                    }
                    None => (),
                }

                // find the one game we want to keep, if any
                for region in &one_regions {
                    let i = games.iter().position(|game| game.regions.contains(region));
                    if i.is_some() {
                        one_region_games.push(games.remove(i.unwrap()));
                        break;
                    }
                }

                // go through the remaining games
                while !games.is_empty() {
                    let game = games.remove(0);
                    if all_regions
                        .iter()
                        .any(|region| game.regions.contains(region))
                    {
                        all_regions_games.push(game);
                    } else {
                        trash_games.push(game);
                    }
                }
            }
        // Regions mode
        } else if !all_regions.is_empty() {
            games = find_games_by_system_id(connection, system.id).await;

            // trim unwanted games
            match unwanted_regex.as_ref() {
                Some(unwanted_regex) => {
                    let (mut unwanted_games, regular_games): (Vec<Game>, Vec<Game>) = games
                        .into_par_iter()
                        .partition(|game| unwanted_regex.find(&game.name).is_some());
                    trash_games.append(&mut unwanted_games);
                    games = regular_games;
                }
                None => (),
            }

            for game in games {
                if all_regions
                    .iter()
                    .any(|region| game.regions.contains(region))
                {
                    all_regions_games.push(game);
                } else {
                    trash_games.push(game);
                }
            }
        } else {
            games = find_games_by_system_id(connection, system.id).await;

            // trim unwanted games
            match unwanted_regex.as_ref() {
                Some(unwanted_regex) => {
                    let (mut unwanted_games, regular_games): (Vec<Game>, Vec<Game>) = games
                        .into_par_iter()
                        .partition(|game| unwanted_regex.find(&game.name).is_some());
                    trash_games.append(&mut unwanted_games);
                    games = regular_games;
                }
                None => (),
            }

            all_regions_games.append(&mut games);
        }

        if matches.is_present("MISSING") {
            progress_bar.set_message("Processing missing games");
            let mut game_ids: Vec<i64> = Vec::new();
            game_ids.append(&mut all_regions_games.iter().map(|game| game.id).collect());
            game_ids.append(&mut one_region_games.iter().map(|game| game.id).collect());
            let missing_roms: Vec<Rom> =
                find_roms_without_romfile_by_game_ids(connection, &game_ids).await;

            progress_bar.println("Missing:");
            for rom in missing_roms {
                progress_bar.println(&format!("{} [{}]", rom.name, rom.crc.to_uppercase()));
            }
        }

        // create necessary directories
        let all_regions_directory = get_rom_directory(connection).await.join(&system.name);
        let one_region_directory = all_regions_directory.join("1G1R");
        let trash_directory = all_regions_directory.join("Trash");
        for d in vec![
            &all_regions_directory,
            &one_region_directory,
            &trash_directory,
        ] {
            create_directory(&d).await?;
        }

        // process all_region_games
        romfile_moves.append(
            &mut process_games(
                connection,
                all_regions_games,
                &all_regions_directory,
                &romfiles_by_id,
            )
            .await,
        );

        // process one_region_games
        romfile_moves.append(
            &mut process_games(
                connection,
                one_region_games,
                &one_region_directory,
                &romfiles_by_id,
            )
            .await,
        );

        // process trash_games
        romfile_moves.append(
            &mut process_games(connection, trash_games, &trash_directory, &romfiles_by_id).await,
        );

        // sort moves and print a summary
        romfile_moves.sort_by(|a, b| a.1.cmp(&b.1));
        romfile_moves.dedup_by(|a, b| a.1 == b.1);

        progress_bar.println("Summary:");
        for file_move in &romfile_moves {
            progress_bar.println(&format!("{} -> {}", file_move.0.path, file_move.1));
        }

        // prompt user for confirmation
        if prompt_for_yes_no(matches).await {
            for romfile_move in romfile_moves {
                let old_path = Path::new(&romfile_move.0.path).to_path_buf();
                let new_path = Path::new(&romfile_move.1).to_path_buf();
                rename_file(&old_path, &new_path).await?;
                update_romfile(connection, romfile_move.0.id, &romfile_move.1).await;
            }
        }
    }

    Ok(())
}

async fn process_games<'a>(
    connection: &mut SqliteConnection,
    games: Vec<Game>,
    directory: &PathBuf,
    romfiles_by_id: &'a HashMap<i64, Romfile>,
) -> Vec<(&'a Romfile, String)> {
    let mut romfile_moves: Vec<(&Romfile, String)> = Vec::new();

    let roms =
        find_roms_with_romfile_by_game_ids(connection, &games.iter().map(|game| game.id).collect())
            .await;

    let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
    roms.into_iter().for_each(|rom| {
        let group = roms_by_game_id.entry(rom.game_id).or_insert(vec![]);
        group.push(rom);
    });

    for game in games {
        let roms = roms_by_game_id.get(&game.id).unwrap();
        let rom_count = roms.len();
        romfile_moves.append(
            &mut roms
                .into_par_iter()
                .map(|rom| {
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    let new_path = String::from(
                        get_new_path(&game, &rom, &romfile, rom_count, &directory)
                            .as_os_str()
                            .to_str()
                            .unwrap(),
                    );
                    return (romfile, new_path);
                })
                .filter(|(romfile, new_path)| &romfile.path != new_path)
                .collect(),
        );
    }

    return romfile_moves;
}

async fn do_discard(
    connection: &mut SqliteConnection,
    matches: &ArgMatches<'_>,
    name: &str,
) -> bool {
    let flags = (
        matches.is_present(&format!("WITH_{}", name)),
        matches.is_present(&format!("WITHOUT_{}", name)),
    );

    if flags.0 ^ flags.1 {
        flags.1
    } else {
        get_bool(connection, &format!("DISCARD_{}", name)).await
    }
}

async fn compute_unwanted_regex(
    connection: &mut SqliteConnection,
    matches: &ArgMatches<'_>,
) -> Option<Regex> {
    let mut unwanted_keywords: Vec<&str> = Vec::new();

    if do_discard(connection, matches, "BETA").await {
        unwanted_keywords.push("Beta( [0-9]+)?");
    }

    if do_discard(connection, matches, "DEBUG").await {
        unwanted_keywords.push("Debug");
    }

    if do_discard(connection, matches, "DEMO").await {
        unwanted_keywords.push("Demo");
    }

    if do_discard(connection, matches, "PROGRAM").await {
        unwanted_keywords.push("Program");
    }

    if do_discard(connection, matches, "PROTO").await {
        unwanted_keywords.push("Proto( [0-9]+)?");
    }

    if do_discard(connection, matches, "SAMPLE").await {
        unwanted_keywords.push("Sample");
    }

    if do_discard(connection, matches, "SEGA_CHANNEL").await {
        unwanted_keywords.push("Sega Channel");
    }

    if do_discard(connection, matches, "VIRTUAL_CONSOLE").await {
        unwanted_keywords.push("([A-z ]+)?Virtual Console");
    }

    // compile unwanted regex
    if !unwanted_keywords.is_empty() {
        Some(Regex::new(&format!(r"\((({})(, )?)+\)", unwanted_keywords.join("|"))).unwrap())
    } else {
        None
    }
}

fn get_new_path(
    game: &Game,
    rom: &Rom,
    romfile: &Romfile,
    rom_count: usize,
    directory: &PathBuf,
) -> PathBuf {
    let romfile_path = Path::new(&romfile.path).to_path_buf();
    let romfile_extension = romfile_path.extension().unwrap().to_str().unwrap();
    let mut new_romfile_path: PathBuf;

    if ARCHIVE_EXTENSIONS.contains(&romfile_extension) {
        let mut romfile_name = match rom_count {
            1 => OsString::from(&rom.name),
            _ => OsString::from(&game.name),
        };
        romfile_name.push(".");
        romfile_name.push(&romfile_extension);
        new_romfile_path = directory.join(romfile_name);
    } else if romfile_extension == CHD_EXTENSION {
        if rom_count == 2 {
            new_romfile_path = directory.join(&rom.name);
            new_romfile_path.set_extension(&romfile_extension);
        } else {
            let mut romfile_name = OsString::from(&game.name);
            romfile_name.push(".");
            romfile_name.push(&romfile_extension);
            new_romfile_path = directory.join(romfile_name);
        }
    } else if romfile_extension == CSO_EXTENSION {
        new_romfile_path = directory.join(&rom.name);
        new_romfile_path.set_extension(&romfile_extension);
    } else {
        new_romfile_path = directory.join(&rom.name);
    }
    new_romfile_path
}

#[cfg(test)]
mod test {
    use super::super::config::set_bool;
    use super::super::database::*;
    use super::super::embedded;
    use super::*;
    use refinery::config::{Config, ConfigDbType};
    use tempfile::NamedTempFile;

    #[async_std::test]
    async fn test_do_discard_with_flag_should_return_false() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let mut config =
            Config::new(ConfigDbType::Sqlite).set_db_path(db_file.path().to_str().unwrap());
        embedded::migrations::runner().run(&mut config).unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let matches = subcommand().get_matches_from(vec!["config", "--with-beta"]);

        // when
        let result = do_discard(&mut connection, &matches, "BETA").await;

        // then
        assert_eq!(false, result);
    }

    #[async_std::test]
    async fn test_do_discard_without_flag_should_return_true() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let mut config =
            Config::new(ConfigDbType::Sqlite).set_db_path(db_file.path().to_str().unwrap());
        embedded::migrations::runner().run(&mut config).unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let matches = subcommand().get_matches_from(vec!["config", "--without-beta"]);

        // when
        let result = do_discard(&mut connection, &matches, "BETA").await;

        // then
        assert_eq!(true, result);
    }

    #[async_std::test]
    async fn test_do_discard_no_flag_should_return_false_from_db() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let mut config =
            Config::new(ConfigDbType::Sqlite).set_db_path(db_file.path().to_str().unwrap());
        embedded::migrations::runner().run(&mut config).unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let matches = subcommand().get_matches_from(vec!["config"]);

        // when
        let result = do_discard(&mut connection, &matches, "BETA").await;

        // then
        assert_eq!(false, result);
    }

    #[async_std::test]
    async fn test_do_discard_no_flag_should_return_true_from_db() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let mut config =
            Config::new(ConfigDbType::Sqlite).set_db_path(db_file.path().to_str().unwrap());
        embedded::migrations::runner().run(&mut config).unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let matches = subcommand().get_matches_from(vec!["config"]);

        // when
        set_bool(&mut connection, "DISCARD_BETA", true).await;
        let result = do_discard(&mut connection, &matches, "BETA").await;

        // then
        assert_eq!(true, result);
    }
}

use super::chdman::*;
use super::config::*;
use super::database::*;
use super::maxcso::*;
use super::model::*;
use super::prompt::*;
use super::sevenzip::*;
use super::util::*;
use super::SimpleResult;
use async_std::path::{Path, PathBuf};
use clap::{App, Arg, ArgMatches, SubCommand};
use indicatif::ProgressBar;
use rayon::prelude::*;
use shiratsu_naming::naming::nointro::{NoIntroName, NoIntroToken};
use shiratsu_naming::naming::{FlagType, TokenizedName};
use shiratsu_naming::region::Region;
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

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches<'_>,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let systems = prompt_for_systems(connection, matches.is_present("ALL"), &progress_bar).await;

    // unordered regions to keep
    let mut all_regions: Vec<Region> = Vec::new();
    if matches.is_present("REGIONS") {
        all_regions = matches
            .values_of("REGIONS")
            .unwrap()
            .map(|r| Region::try_from_tosec_region(r).expect("Failed to parse region code"))
            .flatten()
            .collect();
        all_regions.dedup();
    }

    // ordered regions to use for 1G1R
    let mut one_regions: Vec<Region> = Vec::new();
    if matches.is_present("1G1R") {
        one_regions = matches
            .values_of("1G1R")
            .unwrap()
            .map(|r| Region::try_from_tosec_region(r).expect("Failed to parse region code"))
            .flatten()
            .collect();
        one_regions.dedup();
    }

    // unwanted tokens
    let unwanted_tokens = compute_unwanted_tokens(connection, matches).await;

    for system in systems {
        sort_system(
            connection,
            matches,
            &system,
            &all_regions,
            &one_regions,
            &unwanted_tokens,
            &progress_bar,
        )
        .await?;
    }

    Ok(())
}

pub async fn sort_system<'a>(
    connection: &mut SqliteConnection,
    matches: &ArgMatches<'_>,
    system: &System,
    all_regions: &[Region],
    one_regions: &[Region],
    unwanted_tokens: &Vec<NoIntroToken<'a>>,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    progress_bar.println(&format!("Processing {}", system.name));

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
            if clone_games_by_parent_id.contains_key(&parent.id) {
                games = clone_games_by_parent_id.remove(&parent.id).unwrap();
            } else {
                games = Vec::new();
            }
            games.insert(0, parent);

            // trim unwanted games
            if !unwanted_tokens.is_empty() {
                let (mut unwanted_games, regular_games) = trim_games(games, unwanted_tokens);
                trash_games.append(&mut unwanted_games);
                games = regular_games;
            }

            // find the one game we want to keep, if any
            for region in one_regions {
                let i = games.iter().position(|game| {
                    Region::try_from_tosec_region(&game.regions)
                        .unwrap()
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
                if all_regions.iter().any(|region| {
                    Region::try_from_tosec_region(&game.regions)
                        .unwrap()
                        .contains(region)
                }) {
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
        if !unwanted_tokens.is_empty() {
            let (mut unwanted_games, regular_games) = trim_games(games, unwanted_tokens);
            trash_games.append(&mut unwanted_games);
            games = regular_games;
        }

        for game in games {
            if all_regions.iter().any(|region| {
                Region::try_from_tosec_region(&game.regions)
                    .unwrap()
                    .contains(region)
            }) {
                all_regions_games.push(game);
            } else {
                trash_games.push(game);
            }
        }
    } else {
        games = find_games_by_system_id(connection, system.id).await;

        // trim unwanted games
        if !unwanted_tokens.is_empty() {
            let (mut unwanted_games, regular_games) = trim_games(games, unwanted_tokens);
            trash_games.append(&mut unwanted_games);
            games = regular_games;
        }

        all_regions_games.append(&mut games);
    }

    if matches.is_present("MISSING") {
        let mut game_ids: Vec<i64> = Vec::new();
        game_ids.append(&mut all_regions_games.iter().map(|game| game.id).collect());
        game_ids.append(&mut one_region_games.iter().map(|game| game.id).collect());
        let missing_roms: Vec<Rom> =
            find_roms_without_romfile_by_game_ids(connection, &game_ids).await;

        if !missing_roms.is_empty() {
            progress_bar.println("Missing:");
            for rom in missing_roms {
                progress_bar.println(&format!("{} [{}]", rom.name, rom.crc));
            }
        } else {
            progress_bar.println("No missing ROMs");
        }
    }

    // create necessary directories
    let all_regions_directory = get_rom_directory(connection).await.join(&system.name);
    let one_region_directory = all_regions_directory.join("1G1R");
    let trash_directory = all_regions_directory.join("Trash");
    for d in &[
        &all_regions_directory,
        &one_region_directory,
        &trash_directory,
    ] {
        create_directory(&d).await?;
    }

    // process all_region_games
    romfile_moves.append(
        &mut sort_games(
            connection,
            all_regions_games,
            &all_regions_directory,
            &romfiles_by_id,
        )
        .await,
    );

    // process one_region_games
    romfile_moves.append(
        &mut sort_games(
            connection,
            one_region_games,
            &one_region_directory,
            &romfiles_by_id,
        )
        .await,
    );

    // process trash_games
    romfile_moves
        .append(&mut sort_games(connection, trash_games, &trash_directory, &romfiles_by_id).await);

    if !romfile_moves.is_empty() {
        // sort moves and print a summary
        romfile_moves.sort_by(|a, b| a.1.cmp(&b.1));
        romfile_moves.dedup_by(|a, b| a.1 == b.1);

        progress_bar.println("Summary:");
        for romfile_move in &romfile_moves {
            progress_bar.println(&format!("{} -> {}", romfile_move.0.path, romfile_move.1));
        }

        // prompt user for confirmation
        if prompt_for_yes_no(matches, progress_bar).await {
            for romfile_move in romfile_moves {
                let old_path = Path::new(&romfile_move.0.path).to_path_buf();
                let new_path = Path::new(&romfile_move.1).to_path_buf();
                rename_file(&old_path, &new_path).await?;
                update_romfile(connection, romfile_move.0.id, &romfile_move.1).await;
            }
        }
    } else {
        progress_bar.println("Nothing to do");
    }

    Ok(())
}

async fn sort_games<'a>(
    connection: &mut SqliteConnection,
    games: Vec<Game>,
    directory: &PathBuf,
    romfiles_by_id: &'a HashMap<i64, Romfile>,
) -> Vec<(&'a Romfile, String)> {
    let mut romfile_moves: Vec<(&Romfile, String)> = Vec::new();

    let roms = find_roms_with_romfile_by_game_ids(
        connection,
        &games
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
                    (romfile, new_path)
                })
                .filter(|(romfile, new_path)| &romfile.path != new_path)
                .collect(),
        );
    }

    romfile_moves
}

fn trim_games<'a>(
    games: Vec<Game>,
    unwanted_tokens: &Vec<NoIntroToken<'a>>,
) -> (Vec<Game>, Vec<Game>) {
    games.into_par_iter().partition(|game| {
        if let Ok(name) = NoIntroName::try_parse(&game.name) {
            for token in name.iter() {
                if unwanted_tokens.contains(token) {
                    return true;
                }
            }
        }
        false
    })
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

async fn compute_unwanted_tokens<'a>(
    connection: &mut SqliteConnection,
    matches: &ArgMatches<'_>,
) -> Vec<NoIntroToken<'a>> {
    let mut unwanted_tokens: Vec<NoIntroToken> = Vec::new();

    if do_discard(connection, matches, "BETA").await {
        unwanted_tokens.push(NoIntroToken::Release("Beta", None));
        unwanted_tokens.push(NoIntroToken::Release("Beta", Some("1")));
        unwanted_tokens.push(NoIntroToken::Release("Beta", Some("2")));
        unwanted_tokens.push(NoIntroToken::Release("Beta", Some("3")));
    }

    if do_discard(connection, matches, "CASTLEVANIA_ANNIVERSARY_COLLECTION").await {
        unwanted_tokens.push(NoIntroToken::Flag(
            FlagType::Parenthesized,
            "Castlevania Anniversary Collection",
        ));
    }

    if do_discard(connection, matches, "CLASSIC_MINI").await {
        unwanted_tokens.push(NoIntroToken::Flag(FlagType::Parenthesized, "Classic Mini"));
        unwanted_tokens.push(NoIntroToken::Flag(
            FlagType::Parenthesized,
            "Classic Mini, Switch Online",
        ));
        unwanted_tokens.push(NoIntroToken::Flag(
            FlagType::Parenthesized,
            "Virtual Console, Classic Mini, Switch Online",
        ));
    }

    if do_discard(connection, matches, "DEBUG").await {
        unwanted_tokens.push(NoIntroToken::Flag(FlagType::Parenthesized, "Debug"));
        unwanted_tokens.push(NoIntroToken::Flag(FlagType::Parenthesized, "Debug Version"));
    }

    if do_discard(connection, matches, "DEMO").await {
        unwanted_tokens.push(NoIntroToken::Release("Demo", None));
    }

    if do_discard(connection, matches, "GOG").await {
        unwanted_tokens.push(NoIntroToken::Flag(FlagType::Parenthesized, "GOG"));
    }

    if do_discard(connection, matches, "PROGRAM").await {
        unwanted_tokens.push(NoIntroToken::Flag(FlagType::Parenthesized, "Program"));
    }

    if do_discard(connection, matches, "PROTO").await {
        unwanted_tokens.push(NoIntroToken::Release("Proto", None));
        unwanted_tokens.push(NoIntroToken::Release("Proto", Some("1")));
        unwanted_tokens.push(NoIntroToken::Release("Proto", Some("2")));
        unwanted_tokens.push(NoIntroToken::Release("Proto", Some("3")));
    }

    if do_discard(connection, matches, "SAMPLE").await {
        unwanted_tokens.push(NoIntroToken::Release("Sample", None));
        unwanted_tokens.push(NoIntroToken::Release("Sample", Some("1")));
        unwanted_tokens.push(NoIntroToken::Release("Sample", Some("2")));
        unwanted_tokens.push(NoIntroToken::Release("Sample", Some("3")));
    }

    if do_discard(connection, matches, "SEGA_CHANNEL").await {
        unwanted_tokens.push(NoIntroToken::Flag(FlagType::Parenthesized, "Sega Channel"));
    }

    if do_discard(connection, matches, "SNES_MINI").await {
        unwanted_tokens.push(NoIntroToken::Flag(FlagType::Parenthesized, "SNES Mini"));
        unwanted_tokens.push(NoIntroToken::Flag(
            FlagType::Parenthesized,
            "SNES Mini, Switch Online",
        ));
    }

    if do_discard(connection, matches, "SONIC_CLASSIC_COLLECTION").await {
        unwanted_tokens.push(NoIntroToken::Flag(
            FlagType::Parenthesized,
            "Sonic Classic Collection",
        ));
    }

    if do_discard(connection, matches, "SWITCH_ONLINE").await {
        unwanted_tokens.push(NoIntroToken::Flag(
            FlagType::Parenthesized,
            "Classic Mini, Switch Online",
        ));
        unwanted_tokens.push(NoIntroToken::Flag(
            FlagType::Parenthesized,
            "SNES Mini, Switch Online",
        ));
        unwanted_tokens.push(NoIntroToken::Flag(FlagType::Parenthesized, "Switch Online"));
        unwanted_tokens.push(NoIntroToken::Flag(
            FlagType::Parenthesized,
            "Virtual Console, Switch Online",
        ));
    }

    if do_discard(connection, matches, "VIRTUAL_CONSOLE").await {
        unwanted_tokens.push(NoIntroToken::Flag(
            FlagType::Parenthesized,
            "3DS Virtual Console",
        ));
        unwanted_tokens.push(NoIntroToken::Flag(
            FlagType::Parenthesized,
            "Virtual Console",
        ));
        unwanted_tokens.push(NoIntroToken::Flag(
            FlagType::Parenthesized,
            "Virtual Console, Classic Mini, Switch Online",
        ));
        unwanted_tokens.push(NoIntroToken::Flag(
            FlagType::Parenthesized,
            "Virtual Console, Switch Online",
        ));
        unwanted_tokens.push(NoIntroToken::Flag(
            FlagType::Parenthesized,
            "Wii U Virtual Console",
        ));
        unwanted_tokens.push(NoIntroToken::Flag(
            FlagType::Parenthesized,
            "Wii and Wii U Virtual Console",
        ));
        unwanted_tokens.push(NoIntroToken::Flag(
            FlagType::Parenthesized,
            "Wii Virtual Console",
        ));
    }

    if do_discard(connection, matches, "WII").await {
        unwanted_tokens.push(NoIntroToken::Flag(FlagType::Parenthesized, "Wii"));
    }

    unwanted_tokens.dedup();
    unwanted_tokens
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
    use super::super::config::{set_bool, set_rom_directory, MUTEX};
    use super::super::database::*;
    use super::super::import_dats::import_dat;
    use super::super::import_roms::import_rom;
    use super::super::util::*;
    use super::*;
    use async_std::fs;
    use async_std::sync::Mutex;
    use tempfile::{NamedTempFile, TempDir};

    #[async_std::test]
    async fn test_do_discard_with_flag_should_return_false() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let matches = subcommand().get_matches_from(vec!["config", "--with-beta"]);

        // when
        let result = do_discard(&mut connection, &matches, "BETA").await;

        // then
        assert_eq!(result, false);
    }

    #[async_std::test]
    async fn test_do_discard_without_flag_should_return_true() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let matches = subcommand().get_matches_from(vec!["config", "--without-beta"]);

        // when
        let result = do_discard(&mut connection, &matches, "BETA").await;

        // then
        assert_eq!(result, true);
    }

    #[async_std::test]
    async fn test_do_discard_no_flag_should_return_false_from_db() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let matches = subcommand().get_matches_from(vec!["config"]);

        // when
        let result = do_discard(&mut connection, &matches, "BETA").await;

        // then
        assert_eq!(result, false);
    }

    #[async_std::test]
    async fn test_do_discard_no_flag_should_return_true_from_db() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let matches = subcommand().get_matches_from(vec!["config"]);

        // when
        set_bool(&mut connection, "DISCARD_BETA", true).await;
        set_bool(
            &mut connection,
            "DISCARD_CASTLEVANIA_ANNIVERSARY_COLLECTION",
            true,
        )
        .await;
        let result = do_discard(&mut connection, &matches, "BETA").await;

        // then
        assert_eq!(result, true);
    }

    #[async_std::test]
    async fn test_trim_games_should_discard_everything() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let matches = subcommand().get_matches_from(vec!["config"]);

        set_bool(&mut connection, "DISCARD_BETA", true).await;
        set_bool(
            &mut connection,
            "DISCARD_CASTLEVANIA_ANNIVERSARY_COLLECTION",
            true,
        )
        .await;
        set_bool(&mut connection, "DISCARD_CLASSIC_MINI", true).await;
        set_bool(&mut connection, "DISCARD_DEBUG", true).await;
        set_bool(&mut connection, "DISCARD_DEMO", true).await;
        set_bool(&mut connection, "DISCARD_GOG", true).await;
        set_bool(&mut connection, "DISCARD_PROGRAM", true).await;
        set_bool(&mut connection, "DISCARD_PROTO", true).await;
        set_bool(&mut connection, "DISCARD_SAMPLE", true).await;
        set_bool(&mut connection, "DISCARD_SEGA_CHANNEL", true).await;
        set_bool(&mut connection, "DISCARD_SNES_MINI", true).await;
        set_bool(&mut connection, "DISCARD_SONIC_CLASSIC_COLLECTION", true).await;
        set_bool(&mut connection, "DISCARD_SWITCH_ONLINE", true).await;
        set_bool(&mut connection, "DISCARD_VIRTUAL_CONSOLE", true).await;
        set_bool(&mut connection, "DISCARD_WII", true).await;
        let unwanted_tokens = compute_unwanted_tokens(&mut connection, &matches).await;

        let games = vec![
            Game {
                id: 1,
                name: String::from("Game (USA)"),
                description: String::from(""),
                regions: String::from(""),
                system_id: 1,
                parent_id: None,
            },
            Game {
                id: 2,
                name: String::from("Game (USA) (Beta)"),
                description: String::from(""),
                regions: String::from(""),
                system_id: 1,
                parent_id: None,
            },
            Game {
                id: 3,
                name: String::from("Game (USA) (Beta 1)"),
                description: String::from(""),
                regions: String::from(""),
                system_id: 1,
                parent_id: None,
            },
            Game {
                id: 4,
                name: String::from("Game (USA) (Castlevania Anniversary Collection)"),
                description: String::from(""),
                regions: String::from(""),
                system_id: 1,
                parent_id: None,
            },
            Game {
                id: 5,
                name: String::from("Game (USA) (Classic Mini)"),
                description: String::from(""),
                regions: String::from(""),
                system_id: 1,
                parent_id: None,
            },
            Game {
                id: 6,
                name: String::from("Game (USA) (Debug)"),
                description: String::from(""),
                regions: String::from(""),
                system_id: 1,
                parent_id: None,
            },
            Game {
                id: 7,
                name: String::from("Game (USA) (Demo)"),
                description: String::from(""),
                regions: String::from(""),
                system_id: 1,
                parent_id: None,
            },
            Game {
                id: 8,
                name: String::from("Game (USA) (GOG)"),
                description: String::from(""),
                regions: String::from(""),
                system_id: 1,
                parent_id: None,
            },
            Game {
                id: 9,
                name: String::from("Game (USA) (Program)"),
                description: String::from(""),
                regions: String::from(""),
                system_id: 1,
                parent_id: None,
            },
            Game {
                id: 10,
                name: String::from("Game (USA) (Proto)"),
                description: String::from(""),
                regions: String::from(""),
                system_id: 1,
                parent_id: None,
            },
            Game {
                id: 11,
                name: String::from("Game (USA) (Proto 1)"),
                description: String::from(""),
                regions: String::from(""),
                system_id: 1,
                parent_id: None,
            },
            Game {
                id: 12,
                name: String::from("Game (USA) (Sample)"),
                description: String::from(""),
                regions: String::from(""),
                system_id: 1,
                parent_id: None,
            },
            Game {
                id: 13,
                name: String::from("Game (USA) (Sample 1)"),
                description: String::from(""),
                regions: String::from(""),
                system_id: 1,
                parent_id: None,
            },
            Game {
                id: 14,
                name: String::from("Game (USA) (Sega Channel)"),
                description: String::from(""),
                regions: String::from(""),
                system_id: 1,
                parent_id: None,
            },
            Game {
                id: 15,
                name: String::from("Game (USA) (SNES Mini)"),
                description: String::from(""),
                regions: String::from(""),
                system_id: 1,
                parent_id: None,
            },
            Game {
                id: 16,
                name: String::from("Game (USA) (Sonic Classic Collection)"),
                description: String::from(""),
                regions: String::from(""),
                system_id: 1,
                parent_id: None,
            },
            Game {
                id: 17,
                name: String::from("Game (USA) (Switch Online)"),
                description: String::from(""),
                regions: String::from(""),
                system_id: 1,
                parent_id: None,
            },
            Game {
                id: 18,
                name: String::from("Game (USA) (Virtual Console)"),
                description: String::from(""),
                regions: String::from(""),
                system_id: 1,
                parent_id: None,
            },
            Game {
                id: 19,
                name: String::from("Game (USA) (Wii)"),
                description: String::from(""),
                regions: String::from(""),
                system_id: 1,
                parent_id: None,
            },
        ];
        // when
        let (unwanted_games, regular_games) = trim_games(games, &unwanted_tokens);

        // then
        assert_eq!(unwanted_games.len(), 18);
        assert_eq!(regular_games.len(), 1);
        assert_eq!(regular_games.get(0).unwrap().name, "Game (USA)")
    }

    #[async_std::test]
    async fn test_get_new_path_archive_single_file() {
        // given
        let test_directory = Path::new("test");
        let game = Game {
            id: 1,
            name: String::from("game name"),
            description: String::from(""),
            regions: String::from(""),
            system_id: 1,
            parent_id: None,
        };
        let rom = Rom {
            id: 1,
            name: String::from("rom name.rom"),
            size: 1,
            crc: String::from(""),
            md5: String::from(""),
            sha1: String::from(""),
            rom_status: None,
            game_id: 1,
            romfile_id: Some(1),
        };
        let romfile = Romfile {
            id: 1,
            path: String::from("romfile.7z"),
        };

        // when
        let path = get_new_path(&game, &rom, &romfile, 1, &test_directory.to_path_buf());

        // then
        assert_eq!(path, test_directory.join("rom name.rom.7z"));
    }

    #[async_std::test]
    async fn test_get_new_path_archive_multiple_files() {
        // given
        let test_directory = Path::new("test");
        let game = Game {
            id: 1,
            name: String::from("game name"),
            description: String::from(""),
            regions: String::from(""),
            system_id: 1,
            parent_id: None,
        };
        let rom = Rom {
            id: 1,
            name: String::from("rom name.rom"),
            size: 1,
            crc: String::from(""),
            md5: String::from(""),
            sha1: String::from(""),
            rom_status: None,
            game_id: 1,
            romfile_id: Some(1),
        };
        let romfile = Romfile {
            id: 1,
            path: String::from("romfile.7z"),
        };

        // when
        let path = get_new_path(&game, &rom, &romfile, 2, &test_directory.to_path_buf());

        // then
        assert_eq!(path, test_directory.join("game name.7z"));
    }

    #[async_std::test]
    async fn test_get_new_path_chd_single_file() {
        // given
        let test_directory = Path::new("test");
        let game = Game {
            id: 1,
            name: String::from("game name"),
            description: String::from(""),
            regions: String::from(""),
            system_id: 1,
            parent_id: None,
        };
        let rom = Rom {
            id: 1,
            name: String::from("rom name.bin"),
            size: 1,
            crc: String::from(""),
            md5: String::from(""),
            sha1: String::from(""),
            rom_status: None,
            game_id: 1,
            romfile_id: Some(1),
        };
        let romfile = Romfile {
            id: 1,
            path: String::from("romfile.chd"),
        };

        // when
        let path = get_new_path(&game, &rom, &romfile, 2, &test_directory.to_path_buf());

        // then
        assert_eq!(path, test_directory.join("rom name.chd"));
    }

    #[async_std::test]
    async fn test_get_new_path_chd_multiple_files() {
        // given
        let test_directory = Path::new("test");
        let game = Game {
            id: 1,
            name: String::from("game name"),
            description: String::from(""),
            regions: String::from(""),
            system_id: 1,
            parent_id: None,
        };
        let rom = Rom {
            id: 1,
            name: String::from("rom name.bin"),
            size: 1,
            crc: String::from(""),
            md5: String::from(""),
            sha1: String::from(""),
            rom_status: None,
            game_id: 1,
            romfile_id: Some(1),
        };
        let romfile = Romfile {
            id: 1,
            path: String::from("romfile.chd"),
        };

        // when
        let path = get_new_path(&game, &rom, &romfile, 3, &test_directory.to_path_buf());

        // then
        assert_eq!(path, test_directory.join("game name.chd"));
    }

    #[async_std::test]
    async fn test_get_new_path_cso() {
        // given
        let test_directory = Path::new("test");
        let game = Game {
            id: 1,
            name: String::from("game name"),
            description: String::from(""),
            regions: String::from(""),
            system_id: 1,
            parent_id: None,
        };
        let rom = Rom {
            id: 1,
            name: String::from("rom name.iso"),
            size: 1,
            crc: String::from(""),
            md5: String::from(""),
            sha1: String::from(""),
            rom_status: None,
            game_id: 1,
            romfile_id: Some(1),
        };
        let romfile = Romfile {
            id: 1,
            path: String::from("romfile.cso"),
        };

        // when
        let path = get_new_path(&game, &rom, &romfile, 1, &test_directory.to_path_buf());

        // then
        assert_eq!(path, test_directory.join("rom name.cso"));
    }

    #[async_std::test]
    async fn test_get_new_path_other() {
        // given
        let test_directory = Path::new("test");
        let game = Game {
            id: 1,
            name: String::from("game name"),
            description: String::from(""),
            regions: String::from(""),
            system_id: 1,
            parent_id: None,
        };
        let rom = Rom {
            id: 1,
            name: String::from("rom name.rom"),
            size: 1,
            crc: String::from(""),
            md5: String::from(""),
            sha1: String::from(""),
            rom_status: None,
            game_id: 1,
            romfile_id: Some(1),
        };
        let romfile = Romfile {
            id: 1,
            path: String::from("romfile.rom"),
        };

        // when
        let path = get_new_path(&game, &rom, &romfile, 1, &test_directory.to_path_buf());

        // then
        assert_eq!(path, test_directory.join("rom name.rom"));
    }

    #[async_std::test]
    async fn test_sort_roms_keep_all() {
        // given
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let dat_path = test_directory.join("Test System.dat");
        import_dat(&mut connection, &dat_path, false, &progress_bar)
            .await
            .unwrap();

        let romfile_names = vec![
            "Test Game (Asia).rom",
            "Test Game (Japan).rom",
            "Test Game (USA, Europe).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ];

        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        set_rom_directory(PathBuf::from(&tmp_directory.path()));
        let tmp_path = set_tmp_directory(PathBuf::from(&tmp_directory.path()));
        let system_path = &tmp_path.join("Test System");
        create_directory(&system_path).await.unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        for romfile_name in &romfile_names {
            let romfile_path = tmp_path.join(romfile_name);
            fs::copy(
                test_directory.join(romfile_name),
                &romfile_path.as_os_str().to_str().unwrap(),
            )
            .await
            .unwrap();
            import_rom(
                &mut connection,
                &system_path,
                &system,
                &None,
                &romfile_path,
                &progress_bar,
            )
            .await
            .unwrap();
        }

        let matches = subcommand().get_matches_from(vec!["sort-roms", "-y"]);
        let all_regions = vec![];
        let one_regions = vec![];

        // when
        sort_system(
            &mut connection,
            &matches,
            &system,
            &all_regions,
            &one_regions,
            &vec![],
            &progress_bar,
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
                &system_path
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
    async fn test_sort_roms_discard_beta() {
        // given
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let dat_path = test_directory.join("Test System.dat");
        import_dat(&mut connection, &dat_path, false, &progress_bar)
            .await
            .unwrap();

        let romfile_names = vec![
            "Test Game (Asia).rom",
            "Test Game (Japan).rom",
            "Test Game (USA, Europe).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ];

        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_path = set_rom_directory(PathBuf::from(&tmp_directory.path()));
        let system_path = &tmp_path.join("Test System");
        create_directory(&system_path).await.unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        for romfile_name in &romfile_names {
            let romfile_path = tmp_path.join(romfile_name);
            fs::copy(
                test_directory.join(romfile_name),
                &romfile_path.as_os_str().to_str().unwrap(),
            )
            .await
            .unwrap();
            import_rom(
                &mut connection,
                &system_path,
                &system,
                &None,
                &romfile_path,
                &progress_bar,
            )
            .await
            .unwrap();
        }

        let matches = subcommand().get_matches_from(vec!["config", "-y", "--without-beta"]);
        let all_regions = vec![];
        let one_regions = vec![];
        let unwanted_tokens = compute_unwanted_tokens(&mut connection, &matches).await;

        // when
        sort_system(
            &mut connection,
            &matches,
            &system,
            &all_regions,
            &one_regions,
            &unwanted_tokens,
            &progress_bar,
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
                &system_path
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
                &system_path
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
    async fn test_sort_roms_discard_asia() {
        // given
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let dat_path = test_directory.join("Test System.dat");
        import_dat(&mut connection, &dat_path, false, &progress_bar)
            .await
            .unwrap();

        let romfile_names = vec![
            "Test Game (Asia).rom",
            "Test Game (Japan).rom",
            "Test Game (USA, Europe).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ];

        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_path = set_rom_directory(PathBuf::from(&tmp_directory.path()));
        let system_path = &tmp_path.join("Test System");
        create_directory(&system_path).await.unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        for romfile_name in &romfile_names {
            let romfile_path = tmp_path.join(romfile_name);
            fs::copy(
                test_directory.join(romfile_name),
                &romfile_path.as_os_str().to_str().unwrap(),
            )
            .await
            .unwrap();
            import_rom(
                &mut connection,
                &system_path,
                &system,
                &None,
                &romfile_path,
                &progress_bar,
            )
            .await
            .unwrap();
        }

        let matches = subcommand().get_matches_from(vec!["config", "-y"]);
        let all_regions = vec![Region::UnitedStates, Region::Europe, Region::Japan];
        let one_regions = vec![];
        let unwanted_tokens = compute_unwanted_tokens(&mut connection, &matches).await;

        // when
        sort_system(
            &mut connection,
            &matches,
            &system,
            &all_regions,
            &one_regions,
            &unwanted_tokens,
            &progress_bar,
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
                &system_path
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
                &system_path
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
    async fn test_sort_roms_discard_beta_and_asia() {
        // given
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let dat_path = test_directory.join("Test System.dat");
        import_dat(&mut connection, &dat_path, false, &progress_bar)
            .await
            .unwrap();

        let romfile_names = vec![
            "Test Game (Asia).rom",
            "Test Game (Japan).rom",
            "Test Game (USA, Europe).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ];

        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_path = set_rom_directory(PathBuf::from(&tmp_directory.path()));
        let system_path = &tmp_path.join("Test System");
        create_directory(&system_path).await.unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        for romfile_name in &romfile_names {
            let romfile_path = tmp_path.join(romfile_name);
            fs::copy(
                test_directory.join(romfile_name),
                &romfile_path.as_os_str().to_str().unwrap(),
            )
            .await
            .unwrap();
            import_rom(
                &mut connection,
                &system_path,
                &system,
                &None,
                &romfile_path,
                &progress_bar,
            )
            .await
            .unwrap();
        }

        let matches = subcommand().get_matches_from(vec!["config", "-y", "--without-beta"]);
        let all_regions = vec![Region::UnitedStates, Region::Europe, Region::Japan];
        let one_regions = vec![];
        let unwanted_tokens = compute_unwanted_tokens(&mut connection, &matches).await;

        // when
        sort_system(
            &mut connection,
            &matches,
            &system,
            &all_regions,
            &one_regions,
            &unwanted_tokens,
            &progress_bar,
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
                &system_path
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
                &system_path
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
    async fn test_sort_roms_1g1r_with_parent_clone_information() {
        // given
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let dat_path = test_directory.join("Test System (Parent-Clone).dat");
        import_dat(&mut connection, &dat_path, false, &progress_bar)
            .await
            .unwrap();

        let romfile_names = vec![
            "Test Game (Asia).rom",
            "Test Game (Japan).rom",
            "Test Game (USA, Europe).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ];

        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_path = set_rom_directory(PathBuf::from(&tmp_directory.path()));
        let system_path = &tmp_path.join("Test System");
        create_directory(&system_path).await.unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        for romfile_name in &romfile_names {
            let romfile_path = tmp_path.join(romfile_name);
            fs::copy(
                test_directory.join(romfile_name),
                &romfile_path.as_os_str().to_str().unwrap(),
            )
            .await
            .unwrap();
            import_rom(
                &mut connection,
                &system_path,
                &system,
                &None,
                &romfile_path,
                &progress_bar,
            )
            .await
            .unwrap();
        }

        let matches = subcommand().get_matches_from(vec!["config", "-y"]);
        let all_regions = vec![];
        let one_regions = vec![Region::UnitedStates, Region::Europe];
        let unwanted_tokens = compute_unwanted_tokens(&mut connection, &matches).await;

        // when
        sort_system(
            &mut connection,
            &matches,
            &system,
            &all_regions,
            &one_regions,
            &unwanted_tokens,
            &progress_bar,
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
                &system_path
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
                &system_path
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
    async fn test_sort_roms_1g1r_without_parent_clone_information() {
        // given
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let dat_path = test_directory.join("Test System.dat");
        import_dat(&mut connection, &dat_path, false, &progress_bar)
            .await
            .unwrap();

        let romfile_names = vec![
            "Test Game (Asia).rom",
            "Test Game (Japan).rom",
            "Test Game (USA, Europe).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ];

        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_path = set_rom_directory(PathBuf::from(&tmp_directory.path()));
        let system_path = &tmp_path.join("Test System");
        create_directory(&system_path).await.unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        for romfile_name in &romfile_names {
            let romfile_path = tmp_path.join(romfile_name);
            fs::copy(
                test_directory.join(romfile_name),
                &romfile_path.as_os_str().to_str().unwrap(),
            )
            .await
            .unwrap();
            import_rom(
                &mut connection,
                &system_path,
                &system,
                &None,
                &romfile_path,
                &progress_bar,
            )
            .await
            .unwrap();
        }

        let matches = subcommand().get_matches_from(vec!["config", "-y"]);
        let all_regions = vec![];
        let one_regions = vec![Region::UnitedStates, Region::Europe];
        let unwanted_tokens = compute_unwanted_tokens(&mut connection, &matches).await;

        // when
        sort_system(
            &mut connection,
            &matches,
            &system,
            &all_regions,
            &one_regions,
            &unwanted_tokens,
            &progress_bar,
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
                &system_path
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
                &system_path
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
    async fn test_sort_roms_1g1r_with_parent_clone_information_without_asia_without_beta() {
        // given
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let dat_path = test_directory.join("Test System (Parent-Clone).dat");
        import_dat(&mut connection, &dat_path, false, &progress_bar)
            .await
            .unwrap();

        let romfile_names = vec![
            "Test Game (Asia).rom",
            "Test Game (Japan).rom",
            "Test Game (USA, Europe).rom",
            "Test Game (USA, Europe) (Beta).rom",
        ];

        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_path = set_rom_directory(PathBuf::from(&tmp_directory.path()));
        let system_path = &tmp_path.join("Test System");
        create_directory(&system_path).await.unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        for romfile_name in &romfile_names {
            let romfile_path = tmp_path.join(romfile_name);
            fs::copy(
                test_directory.join(romfile_name),
                &romfile_path.as_os_str().to_str().unwrap(),
            )
            .await
            .unwrap();
            import_rom(
                &mut connection,
                &system_path,
                &system,
                &None,
                &romfile_path,
                &progress_bar,
            )
            .await
            .unwrap();
        }

        let matches = subcommand().get_matches_from(vec!["config", "-y", "--without-beta"]);
        let all_regions = vec![Region::Japan];
        let one_regions = vec![Region::UnitedStates, Region::Europe];
        let unwanted_tokens = compute_unwanted_tokens(&mut connection, &matches).await;

        // when
        sort_system(
            &mut connection,
            &matches,
            &system,
            &all_regions,
            &one_regions,
            &unwanted_tokens,
            &progress_bar,
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
                &system_path
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
                &system_path
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
                &system_path
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

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
use shiratsu_naming::naming::TokenizedName;
use shiratsu_naming::region::Region;
use sqlx::SqliteConnection;
use std::collections::HashMap;
use std::ffi::OsString;

pub fn subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name("sort-roms")
        .about("Sorts ROM files according to region and version preferences")
        .arg(
            Arg::with_name("REGIONS_ALL")
                .short("r")
                .long("regions")
                .help("Sets the regions to keep (unordered)")
                .required(false)
                .takes_value(true)
                .multiple(true),
        )
        .arg(
            Arg::with_name("REGIONS_ONE")
                .short("g")
                .long("1g1r")
                .help("Sets the 1G1R regions to keep (ordered)")
                .required(false)
                .takes_value(true)
                .multiple(true),
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
    let systems = prompt_for_systems(connection, matches.is_present("ALL")).await?;

    let all_regions = compute_regions(connection, matches, "REGIONS_ALL").await;
    let one_regions = compute_regions(connection, matches, "REGIONS_ONE").await;
    let unwanted_releases = get_list(connection, "DISCARD_RELEASES").await;
    let unwanted_flags = get_list(connection, "DISCARD_FLAGS").await;

    for system in systems {
        sort_system(
            connection,
            matches,
            &progress_bar,
            &system,
            &all_regions,
            &one_regions,
            &unwanted_releases
                .iter()
                .map(String::as_str)
                .collect::<Vec<&str>>(),
            &unwanted_flags
                .iter()
                .map(String::as_str)
                .collect::<Vec<&str>>(),
        )
        .await?;
    }

    Ok(())
}

pub async fn compute_regions(
    connection: &mut SqliteConnection,
    matches: &ArgMatches<'_>,
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

async fn sort_system<'a>(
    connection: &mut SqliteConnection,
    matches: &ArgMatches<'_>,
    progress_bar: &ProgressBar,
    system: &System,
    all_regions: &[Region],
    one_regions: &[Region],
    unwanted_releases: &[&str],
    unwanted_flags: &[&str],
) -> SimpleResult<()> {
    progress_bar.println(&format!("Processing \"{}\"", system.name));

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
            if !unwanted_releases.is_empty() || !unwanted_flags.is_empty() {
                let (mut unwanted_games, regular_games) =
                    trim_games(games, unwanted_releases, unwanted_flags);
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
                let region_in_all_regions = all_regions.iter().any(|region| {
                    Region::try_from_tosec_region(&game.regions)
                        .unwrap()
                        .contains(region)
                });
                if region_in_all_regions {
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
        if !unwanted_releases.is_empty() || !unwanted_flags.is_empty() {
            let (mut unwanted_games, regular_games) =
                trim_games(games, unwanted_releases, unwanted_flags);
            trash_games.append(&mut unwanted_games);
            games = regular_games;
        }

        for game in games {
            let region_in_all_regions = all_regions.iter().any(|region| {
                Region::try_from_tosec_region(&game.regions)
                    .unwrap()
                    .contains(region)
            });
            if region_in_all_regions {
                all_regions_games.push(game);
            } else {
                trash_games.push(game);
            }
        };
    } else {
        games = find_games_by_system_id(connection, system.id).await;

        // trim unwanted games
        if !unwanted_releases.is_empty() || !unwanted_flags.is_empty() {
            let (mut unwanted_games, regular_games) =
                trim_games(games, unwanted_releases, unwanted_flags);
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
    let all_regions_directory = get_system_directory(connection, system).await?;
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
        if matches.is_present("YES") || confirm(true)? {
            for romfile_move in romfile_moves {
                rename_file(&romfile_move.0.path, &romfile_move.1).await?;
                update_romfile(connection, romfile_move.0.id, &romfile_move.1).await;
            }
        }
    } else {
        progress_bar.println("Nothing to do");
    }

    Ok(())
}

async fn sort_games<'a, P: AsRef<Path>>(
    connection: &mut SqliteConnection,
    games: Vec<Game>,
    directory: &P,
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
                .iter()
                .map(|rom| {
                    let romfile = romfiles_by_id.get(&rom.romfile_id.unwrap()).unwrap();
                    let new_path = String::from(
                        compute_new_path(&game, &rom, &romfile, rom_count, directory)
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

fn trim_games(
    games: Vec<Game>,
    unwanted_releases: &[&str],
    unwanted_flags: &[&str],
) -> (Vec<Game>, Vec<Game>) {
    games.into_par_iter().partition(|game| {
        if let Ok(name) = NoIntroName::try_parse(&game.name) {
            for token in name.iter() {
                if let NoIntroToken::Release(release, _) = token {
                    if unwanted_releases.contains(release) {
                        return true;
                    }
                }
                if let NoIntroToken::Flag(_, flags) = token {
                    for flag in flags.split(", ") {
                        if unwanted_flags.contains(&flag) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    })
}

fn compute_new_path<P: AsRef<Path>>(
    game: &Game,
    rom: &Rom,
    romfile: &Romfile,
    rom_count: usize,
    directory: &P,
) -> PathBuf {
    let romfile_path = Path::new(&romfile.path);
    let romfile_extension = romfile_path.extension().unwrap().to_str().unwrap();
    let mut new_romfile_path: PathBuf;

    if ARCHIVE_EXTENSIONS.contains(&romfile_extension) {
        let mut romfile_name = match rom_count {
            1 => OsString::from(&rom.name),
            _ => OsString::from(&game.name),
        };
        romfile_name.push(".");
        romfile_name.push(&romfile_extension);
        new_romfile_path = directory.as_ref().join(romfile_name);
    } else if romfile_extension == CHD_EXTENSION {
        if rom_count == 2 {
            new_romfile_path = directory.as_ref().join(&rom.name);
            new_romfile_path.set_extension(&romfile_extension);
        } else {
            let mut romfile_name = OsString::from(&game.name);
            romfile_name.push(".");
            romfile_name.push(&romfile_extension);
            new_romfile_path = directory.as_ref().join(romfile_name);
        }
    } else if romfile_extension == CSO_EXTENSION {
        new_romfile_path = directory.as_ref().join(&rom.name);
        new_romfile_path.set_extension(&romfile_extension);
    } else {
        new_romfile_path = directory.as_ref().join(&rom.name);
    }
    new_romfile_path
}

#[cfg(test)]
mod test {
    use super::super::database::*;
    use super::super::import_dats;
    use super::super::import_roms;
    use super::super::util::*;
    use super::*;
    use async_std::fs;
    use async_std::sync::Mutex;
    use tempfile::{NamedTempFile, TempDir};

    #[async_std::test]
    async fn test_compute_regions_all_should_get_from_matches() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let key = "REGIONS_ALL";

        add_to_list(&mut connection, key, "US").await;
        let matches = subcommand().get_matches_from(&["sort-roms", "-y", "-r", "EU"]);

        // when
        let all_regions = compute_regions(&mut connection, &matches, key).await;

        // then
        assert_eq!(all_regions.len(), 1);
        assert_eq!(all_regions.get(0).unwrap(), &Region::Europe);
    }

    #[async_std::test]
    async fn test_compute_regions_all_should_get_from_db() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let key = "REGIONS_ALL";

        add_to_list(&mut connection, key, "US").await;
        let matches = subcommand().get_matches_from(&["sort-roms", "-y"]);

        // when
        let all_regions = compute_regions(&mut connection, &matches, key).await;

        // then
        assert_eq!(all_regions.len(), 1);
        assert_eq!(all_regions.get(0).unwrap(), &Region::UnitedStates);
    }

    #[async_std::test]
    async fn test_compute_regions_one_should_get_from_matches() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let key = "REGIONS_ONE";

        add_to_list(&mut connection, key, "US").await;
        let matches = subcommand().get_matches_from(&["sort-roms", "-y", "-g", "EU"]);

        // when
        let all_regions = compute_regions(&mut connection, &matches, key).await;

        // then
        assert_eq!(all_regions.len(), 1);
        assert_eq!(all_regions.get(0).unwrap(), &Region::Europe);
    }

    #[async_std::test]
    async fn test_compute_regions_one_should_get_from_db() {
        // given
        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let key = "REGIONS_ONE";

        add_to_list(&mut connection, key, "US").await;
        let matches = subcommand().get_matches_from(&["sort-roms", "-y"]);

        // when
        let all_regions = compute_regions(&mut connection, &matches, key).await;

        // then
        assert_eq!(all_regions.len(), 1);
        assert_eq!(all_regions.get(0).unwrap(), &Region::UnitedStates);
    }

    #[async_std::test]
    async fn test_trim_games_should_discard_unwanted_games() {
        // given
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
                name: String::from("Game (USA) (Virtual Console, Switch Online)"),
                description: String::from(""),
                regions: String::from(""),
                system_id: 1,
                parent_id: None,
            },
        ];

        let unwanted_releases = vec!["Beta"];
        let unwanted_flags = vec!["Virtual Console"];

        // when
        let (unwanted_games, regular_games) =
            trim_games(games, &unwanted_releases, &unwanted_flags);

        // then
        assert_eq!(unwanted_games.len(), 3);
        assert_eq!(regular_games.len(), 1);
        assert_eq!(regular_games.get(0).unwrap().name, "Game (USA)")
    }

    #[async_std::test]
    async fn test_compute_new_path_archive_single_file() {
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
        let path = compute_new_path(&game, &rom, &romfile, 1, &test_directory);

        // then
        assert_eq!(path, test_directory.join("rom name.rom.7z"));
    }

    #[async_std::test]
    async fn test_compute_new_path_archive_multiple_files() {
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
        let path = compute_new_path(&game, &rom, &romfile, 2, &test_directory);

        // then
        assert_eq!(path, test_directory.join("game name.7z"));
    }

    #[async_std::test]
    async fn test_compute_new_path_chd_single_file() {
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
        let path = compute_new_path(&game, &rom, &romfile, 2, &test_directory);

        // then
        assert_eq!(path, test_directory.join("rom name.chd"));
    }

    #[async_std::test]
    async fn test_compute_new_path_chd_multiple_files() {
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
        let path = compute_new_path(&game, &rom, &romfile, 3, &test_directory);

        // then
        assert_eq!(path, test_directory.join("game name.chd"));
    }

    #[async_std::test]
    async fn test_compute_new_path_cso() {
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
        let path = compute_new_path(&game, &rom, &romfile, 1, &test_directory);

        // then
        assert_eq!(path, test_directory.join("rom name.cso"));
    }

    #[async_std::test]
    async fn test_compute_new_path_other() {
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
        let path = compute_new_path(&game, &rom, &romfile, 1, &test_directory);

        // then
        assert_eq!(path, test_directory.join("rom name.rom"));
    }

    #[async_std::test]
    async fn test_sort_system_keep_all() {
        // given
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

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

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        let rom_directory = set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));
        let system_directory = &rom_directory.join("Test System");
        create_directory(&system_directory).await.unwrap();

        let system = find_systems(&mut connection).await.remove(0);

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
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

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

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        let rom_directory = set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));
        let system_directory = &rom_directory.join("Test System");
        create_directory(&system_directory).await.unwrap();

        let system = find_systems(&mut connection).await.remove(0);

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
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

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

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        let rom_directory = set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));
        let system_directory = &rom_directory.join("Test System");
        create_directory(&system_directory).await.unwrap();

        let system = find_systems(&mut connection).await.remove(0);

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
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

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

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        let rom_directory = set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));
        let system_directory = &rom_directory.join("Test System");
        create_directory(&system_directory).await.unwrap();

        let system = find_systems(&mut connection).await.remove(0);

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
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

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

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        let rom_directory = set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));
        let system_directory = &rom_directory.join("Test System");
        create_directory(&system_directory).await.unwrap();

        let system = find_systems(&mut connection).await.remove(0);

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
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

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

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        let rom_directory = set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));
        let system_directory = &rom_directory.join("Test System");
        create_directory(&system_directory).await.unwrap();

        let system = find_systems(&mut connection).await.remove(0);

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
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

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

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        let rom_directory = set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));
        let system_directory = &rom_directory.join("Test System");
        create_directory(&system_directory).await.unwrap();

        let system = find_systems(&mut connection).await.remove(0);

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

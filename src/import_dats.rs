use super::checksum::*;
use super::database::*;
use super::import_roms::import_rom;
use super::model::*;
use super::progress::*;
use super::util::*;
use super::SimpleResult;
use async_std::path::Path;
use clap::{Arg, ArgMatches, Command};
use indicatif::ProgressBar;
use quick_xml::de;
use rayon::prelude::*;
use regex::Regex;
use rust_embed::RustEmbed;
use shiratsu_naming::naming::nointro::{NoIntroName, NoIntroToken};
use shiratsu_naming::naming::TokenizedName;
use shiratsu_naming::region::Region;
use sqlx::sqlite::SqliteConnection;
use std::io;
use std::str;
use vec_drain_where::VecDrainWhereExt;

lazy_static! {
    pub static ref SYSTEM_NAME_REGEX: Regex = Regex::new(r"\(.*\)").unwrap();
}

#[derive(RustEmbed)]
#[folder = "data/"]
struct Assets;

pub fn subcommand<'a>() -> Command<'a> {
    Command::new("import-dats")
        .about("Parse and import Logiqx DAT files into oxyromon")
        .arg(
            Arg::new("DATS")
                .help("Set the DAT files to import")
                .required(true)
                .multiple_values(true)
                .index(1)
                .allow_invalid_utf8(true),
        )
        .arg(
            Arg::new("INFO")
                .short('i')
                .long("info")
                .help("Show the DAT information and exit")
                .required(false),
        )
        .arg(
            Arg::new("SKIP_HEADER")
                .short('s')
                .long("skip-header")
                .help("Skip parsing the header even if the system has one")
                .required(false),
        )
        .arg(
            Arg::new("FORCE")
                .short('f')
                .long("force")
                .help("Force import of outdated DAT files")
                .required(false),
        )
        .arg(
            Arg::new("ARCADE")
                .short('a')
                .long("arcade")
                .help("Enable arcade mode")
                .required(false),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let dat_paths: Vec<String> = matches.values_of_lossy("DATS").unwrap();

    for dat_path in dat_paths {
        progress_bar.println(&format!("Processing \"{}\"", &dat_path));
        let (datfile_xml, detector_xml) = parse_dat(
            progress_bar,
            &get_canonicalized_path(&dat_path).await?,
            matches.is_present("SKIP_HEADER"),
        )
        .await?;
        if !matches.is_present("INFO") {
            import_dat(
                connection,
                progress_bar,
                &datfile_xml,
                &detector_xml,
                matches.is_present("ARCADE"),
                matches.is_present("FORCE"),
            )
            .await?;
        }
        progress_bar.println("");
    }

    Ok(())
}

pub async fn parse_dat<P: AsRef<Path>>(
    progress_bar: &ProgressBar,
    dat_path: &P,
    skip_header: bool,
) -> SimpleResult<(DatfileXml, Option<DetectorXml>)> {
    let datfile_xml: DatfileXml = try_with!(
        de::from_reader(&mut get_reader_sync(dat_path)?),
        "Failed to parse DAT file"
    );

    // print information
    progress_bar.println(format!("System: {}", datfile_xml.system.name));
    progress_bar.println(format!("Version: {}", datfile_xml.system.version));
    progress_bar.println(format!("Games: {}", datfile_xml.games.len()));

    let mut detector_xml = None;
    if !skip_header {
        if let Some(clr_mame_pro_xml) = &datfile_xml
            .system
            .clrmamepros
            .iter()
            .find(|clrmamepro| clrmamepro.header.is_some())
        {
            progress_bar.println("Processing header");
            if let Some(header_file_name) = &clr_mame_pro_xml.header {
                let header_file_path = dat_path.as_ref().parent().unwrap().join(header_file_name);
                if header_file_path.is_file().await {
                    let header_file = open_file_sync(&header_file_path.as_path())?;
                    let reader = io::BufReader::new(header_file);
                    detector_xml = de::from_reader(reader).expect("Failed to parse header file");
                } else {
                    let header_file = Assets::get(header_file_name).unwrap();
                    detector_xml = de::from_str(str::from_utf8(header_file.data.as_ref()).unwrap())
                        .expect("Failed to parse header file");
                }
            }
        }
    };

    Ok((datfile_xml, detector_xml))
}

pub async fn import_dat(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    datfile_xml: &DatfileXml,
    detector_xml: &Option<DetectorXml>,
    arcade: bool,
    force: bool,
) -> SimpleResult<()> {
    progress_bar.println("Processing system");

    let mut transaction = begin_transaction(connection).await;

    // persist system
    let system_id = match create_or_update_system(
        &mut transaction,
        progress_bar,
        &datfile_xml.system,
        arcade,
        force,
    )
    .await
    {
        Some(system_id) => system_id,
        None => return Ok(()),
    };

    // persist header
    if let Some(detector_xml) = detector_xml {
        create_or_update_header(&mut transaction, detector_xml, system_id).await;
    }

    progress_bar.reset();
    progress_bar.set_style(get_count_progress_style());
    progress_bar.set_length(datfile_xml.games.len() as u64);

    // persist games
    let mut orphan_romfile_ids: Vec<i64> = Vec::new();
    progress_bar.println("Deleting old games");
    orphan_romfile_ids
        .append(&mut delete_old_games(&mut transaction, &datfile_xml.games, system_id).await);
    progress_bar.println("Processing games");
    orphan_romfile_ids.append(
        &mut create_or_update_games(
            &mut transaction,
            &datfile_xml.games,
            system_id,
            arcade,
            progress_bar,
        )
        .await?,
    );
    if !orphan_romfile_ids.is_empty() {
        progress_bar.println("Processing orphan romfiles");
        reimport_orphan_romfiles(
            &mut transaction,
            progress_bar,
            system_id,
            orphan_romfile_ids,
            &HashAlgorithm::Crc,
        )
        .await?;
    }

    // create necessary directories
    let system = find_system_by_id(&mut transaction, system_id).await;
    get_system_directory(&mut transaction, progress_bar, &system).await?;
    get_trash_directory(&mut transaction, progress_bar, &system).await?;

    // update games and systems completion
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(100);
    progress_bar.set_message("Computing system completion");
    update_games_by_system_id_mark_complete(&mut transaction, system.id).await;
    update_games_by_system_id_mark_incomplete(&mut transaction, system.id).await;
    update_system_mark_complete(&mut transaction, system.id).await;
    update_system_mark_incomplete(&mut transaction, system.id).await;

    commit_transaction(transaction).await;

    Ok(())
}

fn get_regions_from_game_name(name: &str) -> SimpleResult<String> {
    let name = try_with!(
        NoIntroName::try_parse(name),
        "Failed to parse no-intro name"
    );
    for token in name.iter() {
        if let NoIntroToken::Region(_, regions) = token {
            return Ok(Region::to_normalized_region_string(regions));
        }
    }
    Ok(String::from(""))
}

async fn create_or_update_system(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system_xml: &SystemXml,
    arcade: bool,
    force: bool,
) -> Option<i64> {
    match find_system_by_name(connection, &system_xml.name).await {
        Some(system) => {
            if is_update(progress_bar, &system.version, &system_xml.version) || force {
                update_system_from_xml(connection, system.id, system_xml, arcade).await;
                Some(system.id)
            } else {
                None
            }
        }
        None => Some(create_system_from_xml(connection, system_xml, arcade).await),
    }
}

async fn create_or_update_header(
    connection: &mut SqliteConnection,
    detector_xml: &DetectorXml,
    system_id: i64,
) {
    let header_id = match find_header_by_system_id(connection, system_id).await {
        Some(header) => {
            update_header_from_xml(connection, header.id, detector_xml, system_id).await;
            delete_rules_by_header_id(connection, header.id).await;
            header.id
        }
        None => create_header_from_xml(connection, detector_xml, system_id).await,
    };
    for data_xml in &detector_xml.rule.data {
        create_rule_from_xml(connection, data_xml, header_id).await;
    }
}

async fn create_or_update_games(
    connection: &mut SqliteConnection,
    games_xml: &[GameXml],
    system_id: i64,
    arcade: bool,
    progress_bar: &ProgressBar,
) -> SimpleResult<Vec<i64>> {
    let mut orphan_romfile_ids: Vec<i64> = Vec::new();
    let (mut parent_games_xml, mut child_games_xml): (Vec<&GameXml>, Vec<&GameXml>) = games_xml
        .par_iter()
        .partition(|game_xml| game_xml.cloneof.is_none() && game_xml.romof.is_none());
    for game_xml in &parent_games_xml {
        let game = find_game_by_name_and_bios_and_system_id(
            connection,
            &game_xml.name,
            game_xml.isbios.is_some() && game_xml.isbios.as_ref().unwrap() == "yes",
            system_id,
        )
        .await;
        let regions = match arcade {
            false => get_regions_from_game_name(&game_xml.name).unwrap(),
            true => String::new(),
        };
        let game_id = match game {
            Some(game) => {
                update_game_from_xml(
                    connection, game.id, game_xml, &regions, system_id, None, None,
                )
                .await;
                game.id
            }
            None => {
                create_game_from_xml(connection, game_xml, &regions, system_id, None, None).await
            }
        };
        if !game_xml.roms.is_empty() {
            orphan_romfile_ids.append(
                &mut create_or_update_roms(
                    connection,
                    progress_bar,
                    &game_xml.roms,
                    game_xml.isbios.is_some() && game_xml.isbios.as_ref().unwrap() == "yes",
                    game_id,
                )
                .await,
            );
        }
        orphan_romfile_ids.append(&mut delete_old_roms(connection, &game_xml.roms, game_id).await);
        progress_bar.inc(1)
    }
    while !child_games_xml.is_empty() {
        let parent_game_names: Vec<&str> = parent_games_xml
            .par_iter()
            .map(|game_xml| game_xml.name.as_str())
            .collect();
        parent_games_xml = child_games_xml
            .e_drain_where(|&mut child_game_xml| {
                parent_game_names.contains(
                    &child_game_xml
                        .cloneof
                        .as_ref()
                        .or_else(|| child_game_xml.romof.as_ref())
                        .unwrap()
                        .as_str(),
                )
            })
            .collect();
        for game_xml in &parent_games_xml {
            let game = find_game_by_name_and_bios_and_system_id(
                connection,
                &game_xml.name,
                game_xml.isbios.is_some() && game_xml.isbios.as_ref().unwrap() == "yes",
                system_id,
            )
            .await;
            let parent_game = match game_xml.cloneof.as_ref() {
                Some(name) => {
                    find_game_by_name_and_bios_and_system_id(connection, name, false, system_id)
                        .await
                }
                None => None,
            };
            let bios_game: Option<Game> = match game_xml.romof.as_ref() {
                Some(name) => {
                    find_game_by_name_and_bios_and_system_id(connection, name, true, system_id)
                        .await
                }
                None => None,
            };
            let regions = match arcade {
                false => get_regions_from_game_name(&game_xml.name).unwrap(),
                true => String::new(),
            };
            let game_id = match game {
                Some(game) => {
                    update_game_from_xml(
                        connection,
                        game.id,
                        game_xml,
                        &regions,
                        system_id,
                        parent_game.map(|game| game.id),
                        bios_game.map(|game| game.id),
                    )
                    .await;
                    game.id
                }
                None => {
                    create_game_from_xml(
                        connection,
                        game_xml,
                        &regions,
                        system_id,
                        parent_game.map(|game| game.id),
                        bios_game.map(|game| game.id),
                    )
                    .await
                }
            };
            if !game_xml.roms.is_empty() {
                orphan_romfile_ids.append(
                    &mut create_or_update_roms(
                        connection,
                        progress_bar,
                        &game_xml.roms,
                        game_xml.isbios.is_some() && game_xml.isbios.as_ref().unwrap() == "yes",
                        game_id,
                    )
                    .await,
                );
            }
            orphan_romfile_ids
                .append(&mut delete_old_roms(connection, &game_xml.roms, game_id).await);
            progress_bar.inc(1)
        }
    }
    Ok(orphan_romfile_ids)
}

async fn create_or_update_roms(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    roms_xml: &[RomXml],
    mut bios: bool,
    game_id: i64,
) -> Vec<i64> {
    let mut orphan_romfile_ids: Vec<i64> = Vec::new();
    for rom_xml in roms_xml {
        // skip nodump roms
        if rom_xml.status.is_some() && rom_xml.status.as_ref().unwrap() == "nodump" {
            continue;
        }
        // find parent rom if needed
        let mut parent_id = None;
        if rom_xml.merge.is_some() {
            let game = find_game_by_id(connection, game_id).await;
            // try cloneof first, or romof if there is no cloneof
            let mut parent_rom = find_rom_by_size_and_crc_and_game_id(
                connection,
                rom_xml.size,
                rom_xml.crc.as_ref().unwrap(),
                game.parent_id.or(game.bios_id).unwrap(),
            )
            .await;
            // try romof next
            if parent_rom.is_none() && game.bios_id.is_some() {
                parent_rom = find_rom_by_size_and_crc_and_game_id(
                    connection,
                    rom_xml.size,
                    rom_xml.crc.as_ref().unwrap(),
                    game.bios_id.unwrap(),
                )
                .await;
            }
            if parent_rom.is_some() {
                let parent_rom = parent_rom.unwrap();
                bios = parent_rom.bios;
                parent_id = parent_rom.parent_id.or(Some(parent_rom.id));
            } else {
                progress_bar.println(&format!(
                    "Rom \"{}\" not found in game \"{}\" parent/bios, please fix your DAT file",
                    rom_xml.name, game.name
                ));
            }
        }
        match find_rom_by_name_and_game_id(connection, &rom_xml.name, game_id).await {
            Some(rom) => {
                update_rom_from_xml(connection, rom.id, rom_xml, bios, game_id, parent_id).await;
                if rom_xml.size != rom.size
                    || rom_xml.crc.as_ref().unwrap() != rom.crc.as_ref().unwrap()
                {
                    if let Some(romfile_id) = rom.romfile_id {
                        orphan_romfile_ids.push(romfile_id);
                        update_rom_romfile(connection, rom.id, None).await;
                    }
                }
                rom.id
            }
            None => create_rom_from_xml(connection, rom_xml, bios, game_id, parent_id).await,
        };
    }
    orphan_romfile_ids
}

async fn delete_old_games(
    connection: &mut SqliteConnection,
    games_xml: &[GameXml],
    system_id: i64,
) -> Vec<i64> {
    let mut orphan_romfile_ids: Vec<i64> = Vec::new();
    let game_names_xml: Vec<&String> = games_xml.iter().map(|game_xml| &game_xml.name).collect();
    let games: Vec<Game> = find_games_by_system_id(connection, system_id)
        .await
        .into_par_iter()
        .filter(|game| !game_names_xml.contains(&&game.name))
        .collect();
    for game in games {
        orphan_romfile_ids.extend(
            find_roms_by_game_id_no_parents(connection, game.id)
                .await
                .into_iter()
                .filter_map(|rom| rom.romfile_id),
        );
        delete_game_by_name_and_system_id(connection, &game.name, system_id).await;
    }
    orphan_romfile_ids
}

async fn delete_old_roms(
    connection: &mut SqliteConnection,
    roms_xml: &[RomXml],
    game_id: i64,
) -> Vec<i64> {
    let rom_names_romfile_ids: Vec<(String, Option<i64>)> =
        find_roms_by_game_id_no_parents(connection, game_id)
            .await
            .into_par_iter()
            .map(|rom| (rom.name, rom.romfile_id))
            .collect();
    let mut orphan_romfile_ids: Vec<i64> = Vec::new();
    for (rom_name, rom_romfile_id) in &rom_names_romfile_ids {
        if !roms_xml.iter().any(|rom_xml| &rom_xml.name == rom_name) {
            delete_rom_by_name_and_game_id(connection, rom_name, game_id).await;
            if let Some(romfile_id) = rom_romfile_id {
                orphan_romfile_ids.push(*romfile_id);
            }
        }
    }
    orphan_romfile_ids
}

pub async fn reimport_orphan_romfiles(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system_id: i64,
    orphan_romfile_ids: Vec<i64>,
    hash_algorithm: &HashAlgorithm,
) -> SimpleResult<()> {
    let system = find_system_by_id(connection, system_id).await;
    let header = find_header_by_system_id(connection, system_id).await;
    for romfile_id in orphan_romfile_ids {
        let romfile = find_romfile_by_id(connection, romfile_id).await;
        delete_romfile_by_id(connection, romfile_id).await;
        import_rom(
            connection,
            progress_bar,
            &system,
            &header,
            &Path::new(&romfile.path),
            hash_algorithm,
        )
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use super::super::config::*;
    use super::super::database::*;
    use super::*;
    use async_std::fs;
    use async_std::path::PathBuf;
    use tempfile::{NamedTempFile, TempDir};

    #[async_std::test]
    async fn test_import_dat() {
        // given
        let test_directory = Path::new("tests");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let dat_path = test_directory.join("Test System (20200721).dat");
        let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();

        // when
        import_dat(
            &mut connection,
            &progress_bar,
            &datfile_xml,
            &detector_xml,
            false,
            false,
        )
        .await
        .unwrap();

        // then
        let systems = find_systems(&mut connection).await;
        assert_eq!(systems.len(), 1);

        let system = systems.get(0).unwrap();
        assert_eq!(system.name, "Test System");

        assert_eq!(find_games(&mut connection).await.len(), 6);
        assert_eq!(find_roms(&mut connection).await.len(), 8);
    }

    #[async_std::test]
    async fn test_import_dat_parent_clone() {
        // given
        let test_directory = Path::new("tests");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let dat_path = test_directory.join("Test System (20200721) (Parent-Clone).dat");
        let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();

        // when
        import_dat(
            &mut connection,
            &progress_bar,
            &datfile_xml,
            &detector_xml,
            false,
            false,
        )
        .await
        .unwrap();

        // then
        let systems = find_systems(&mut connection).await;
        assert_eq!(systems.len(), 1);

        let system = systems.get(0).unwrap();
        assert_eq!(system.name, "Test System");

        assert_eq!(find_games(&mut connection).await.len(), 4);
        assert_eq!(find_roms(&mut connection).await.len(), 4);
    }

    #[async_std::test]
    async fn test_import_dat_headered() {
        // given
        let test_directory = Path::new("tests");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let dat_path = test_directory.join("Test System (20210402) (Headered).dat");
        let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();

        // when
        import_dat(
            &mut connection,
            &progress_bar,
            &datfile_xml,
            &detector_xml,
            false,
            false,
        )
        .await
        .unwrap();

        // then
        let systems = find_systems(&mut connection).await;
        assert_eq!(systems.len(), 1);

        let system = systems.get(0).unwrap();
        assert_eq!(system.name, "Test System (Headered)");

        assert!(find_header_by_system_id(&mut connection, system.id)
            .await
            .is_some());

        assert_eq!(find_games(&mut connection).await.len(), 1);
        assert_eq!(find_roms(&mut connection).await.len(), 1);
    }

    #[async_std::test]
    async fn test_import_dat_headered_skip_header() {
        // given
        let test_directory = Path::new("tests");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let dat_path = test_directory.join("Test System (20210402) (Headered).dat");
        let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, true).await.unwrap();

        // when
        import_dat(
            &mut connection,
            &progress_bar,
            &datfile_xml,
            &detector_xml,
            false,
            false,
        )
        .await
        .unwrap();

        // then
        let systems = find_systems(&mut connection).await;
        assert_eq!(systems.len(), 1);

        let system = systems.get(0).unwrap();
        assert_eq!(system.name, "Test System (Headered)");

        assert!(find_header_by_system_id(&mut connection, system.id)
            .await
            .is_none());

        assert_eq!(find_games(&mut connection).await.len(), 1);
        assert_eq!(find_roms(&mut connection).await.len(), 1);
    }

    #[async_std::test]
    async fn test_import_dat_headered_embedded() {
        // given
        let test_directory = Path::new("tests");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let dat_path = test_directory.join("Test System (20211124) (Headered) (Embedded).dat");
        let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();

        // when
        import_dat(
            &mut connection,
            &progress_bar,
            &datfile_xml,
            &detector_xml,
            false,
            false,
        )
        .await
        .unwrap();

        // then
        let systems = find_systems(&mut connection).await;
        assert_eq!(systems.len(), 1);

        let system = systems.get(0).unwrap();
        assert_eq!(system.name, "Test System (Headered) (Embedded)");

        assert!(find_header_by_system_id(&mut connection, system.id)
            .await
            .is_some());

        assert_eq!(find_games(&mut connection).await.len(), 1);
        assert_eq!(find_roms(&mut connection).await.len(), 1);
    }

    #[async_std::test]
    async fn test_import_updated_dat() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("tests");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let dat_path = test_directory.join("Test System (20200721).dat");
        let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));

        import_dat(
            &mut connection,
            &progress_bar,
            &datfile_xml,
            &detector_xml,
            false,
            false,
        )
        .await
        .unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        let romfile_names = vec![
            "Test Game (Asia).rom",
            "Test Game (Japan).rom",
            "Test Game (USA, Europe).rom",
        ];
        for romfile_name in romfile_names {
            let romfile_path = tmp_directory.join(romfile_name);
            fs::copy(
                test_directory.join(romfile_name),
                &romfile_path.as_os_str().to_str().unwrap(),
            )
            .await
            .unwrap();
            import_rom(
                &mut connection,
                &progress_bar,
                &system,
                &None,
                &romfile_path,
                &HashAlgorithm::Crc,
            )
            .await
            .unwrap();
        }

        let dat_path = test_directory.join("Test System (20210401).dat");
        let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();

        // when
        import_dat(
            &mut connection,
            &progress_bar,
            &datfile_xml,
            &detector_xml,
            false,
            false,
        )
        .await
        .unwrap();

        // then
        let systems = find_systems(&mut connection).await;
        assert_eq!(systems.len(), 1);

        let system = systems.get(0).unwrap();
        assert_eq!(system.name, "Test System");

        let games = find_games(&mut connection).await;
        let roms = find_roms(&mut connection).await;
        let romfiles = find_romfiles(&mut connection).await;

        assert_eq!(games.len(), 3);
        assert_eq!(roms.len(), 3);
        assert_eq!(romfiles.len(), 3);

        let game = games.get(0).unwrap();
        let rom = roms.get(0).unwrap();
        let romfile = romfiles.get(1).unwrap();

        assert_eq!(game.name, "Test Game (Asia)");
        assert_eq!(rom.name, "Test Game (Asia).rom");
        assert!(rom.romfile_id.is_none());
        assert!(romfile.path.contains("/Trash/"));

        let game = games.get(1).unwrap();
        let rom = roms.get(2).unwrap();
        let romfile = romfiles.get(2).unwrap();

        assert_eq!(game.name, "Test Game (USA, Europe)");
        assert_eq!(rom.name, "Updated Test Game (USA, Europe).rom");
        assert!(rom.romfile_id.is_some());
        assert_eq!(rom.romfile_id.unwrap(), romfile.id);

        let game = games.get(2).unwrap();
        let rom = roms.get(1).unwrap();
        let romfile = romfiles.get(0).unwrap();

        assert_eq!(game.name, "Updated Test Game (Japan)");
        assert_eq!(rom.name, "Test Game (Japan).rom");
        assert!(rom.romfile_id.is_some());
        assert_eq!(rom.romfile_id.unwrap(), romfile.id);
    }

    #[async_std::test]
    async fn test_import_outdated_dat_should_do_nothing() {
        // given
        let test_directory = Path::new("tests");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let dat_path = test_directory.join("Test System (20200721).dat");
        let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();

        import_dat(
            &mut connection,
            &progress_bar,
            &datfile_xml,
            &detector_xml,
            false,
            false,
        )
        .await
        .unwrap();

        let dat_path = test_directory.join("Test System (20000000).dat");
        let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();

        // when
        import_dat(
            &mut connection,
            &progress_bar,
            &datfile_xml,
            &detector_xml,
            false,
            false,
        )
        .await
        .unwrap();

        // then
        let systems = find_systems(&mut connection).await;
        assert_eq!(systems.len(), 1);

        let system = systems.get(0).unwrap();
        assert_eq!(system.name, "Test System");

        assert_eq!(find_games(&mut connection).await.len(), 6);
        assert_eq!(find_roms(&mut connection).await.len(), 8);
    }

    #[async_std::test]
    async fn test_import_outdated_dat_forced_should_import() {
        // given
        let test_directory = Path::new("tests");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let dat_path = test_directory.join("Test System (20200721).dat");
        let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();

        import_dat(
            &mut connection,
            &progress_bar,
            &datfile_xml,
            &detector_xml,
            false,
            false,
        )
        .await
        .unwrap();

        let dat_path = test_directory.join("Test System (20000000).dat");
        let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();

        // when
        import_dat(
            &mut connection,
            &progress_bar,
            &datfile_xml,
            &detector_xml,
            false,
            true,
        )
        .await
        .unwrap();

        // then
        let systems = find_systems(&mut connection).await;
        assert_eq!(systems.len(), 1);

        let system = systems.get(0).unwrap();
        assert_eq!(system.name, "Test System");

        assert_eq!(find_games(&mut connection).await.len(), 3);
        assert_eq!(find_roms(&mut connection).await.len(), 3);
    }

    #[test]
    fn test_get_regions_from_game_name_world() {
        // given
        let game_name = "Test Game (World)";

        // when
        let regions = get_regions_from_game_name(game_name).unwrap();

        // then
        assert_eq!(regions, "US-JP-EU");
    }

    #[test]
    fn test_get_regions_from_game_name_france_germany() {
        // given
        let game_name = "Test Game (France, Germany)";

        // when
        let regions = get_regions_from_game_name(game_name).unwrap();

        // then
        assert_eq!(regions, "FR-DE");
    }

    #[async_std::test]
    async fn test_import_dat_headered_duplicate_clrmamepro() {
        // given
        let test_directory = Path::new("tests");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let dat_path =
            test_directory.join("Test System (20220430) (Headered) (Duplicate Clrmamepro).dat");
        let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();

        // when
        import_dat(
            &mut connection,
            &progress_bar,
            &datfile_xml,
            &detector_xml,
            false,
            false,
        )
        .await
        .unwrap();

        // then
        let systems = find_systems(&mut connection).await;
        assert_eq!(systems.len(), 1);

        let system = systems.get(0).unwrap();
        assert_eq!(system.name, "Test System (Headered)");

        assert!(find_header_by_system_id(&mut connection, system.id)
            .await
            .is_some());

        assert_eq!(find_games(&mut connection).await.len(), 1);
        assert_eq!(find_roms(&mut connection).await.len(), 1);
    }
}

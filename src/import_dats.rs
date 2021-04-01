use super::config::*;
use super::database::*;
use super::import_roms::import_rom;
use super::model::*;
use super::progress::*;
use super::util::*;
use super::SimpleResult;
use async_std::path::{Path, PathBuf};
use clap::{App, Arg, ArgMatches, SubCommand};
use indicatif::ProgressBar;
use quick_xml::de;
use rayon::prelude::*;
use regex::Regex;
use shiratsu_naming::naming::nointro::{NoIntroName, NoIntroToken};
use shiratsu_naming::naming::TokenizedName;
use shiratsu_naming::region::Region;
use sqlx::SqliteConnection;
use std::io;

pub fn subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name("import-dats")
        .about("Parses and imports No-Intro and Redump DAT files into oxyromon")
        .arg(
            Arg::with_name("DATS")
                .help("Sets the DAT files to import")
                .required(true)
                .multiple(true)
                .index(1),
        )
        .arg(
            Arg::with_name("INFO")
                .short("i")
                .long("info")
                .help("Shows the DAT information and exit")
                .required(false),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches<'_>,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let dats: Vec<String> = matches.values_of_lossy("DATS").unwrap();
    let info = matches.is_present("INFO");

    for dat in dats {
        let dat_path = get_canonicalized_path(&dat).await?;
        import_dat(connection, &dat_path, info, &progress_bar).await?;
    }

    Ok(())
}

pub async fn import_dat(
    connection: &mut SqliteConnection,
    dat_path: &PathBuf,
    info: bool,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let f = open_file_sync(&dat_path.into())?;
    let reader = io::BufReader::new(f);
    let mut datafile_xml: DatfileXml =
        de::from_reader(reader).expect("Failed to parse the datafile");

    // strip the parentheses qualifiers from the system name
    let re = Regex::new(r"\(.*\)").unwrap();
    datafile_xml.system.name = re.replace(&datafile_xml.system.name, "").trim().to_owned();

    // print information
    progress_bar.println(format!("System: {}", datafile_xml.system.name));
    progress_bar.println(format!("Version: {}", datafile_xml.system.version));
    progress_bar.println(format!("Games: {}", datafile_xml.games.len()));

    if info {
        return Ok(());
    }

    progress_bar.reset();
    progress_bar.set_style(get_count_progress_style());
    progress_bar.set_length(datafile_xml.games.len() as u64);

    // persist system
    progress_bar.println("Processing system");
    let system_id = create_or_update_system(connection, &datafile_xml.system).await;

    // parse header file if needed
    if datafile_xml.system.clrmamepro.is_some() {
        progress_bar.println("Processing header");
        let header_file_name = &datafile_xml.system.clrmamepro.unwrap().header;
        let header_file_path = dat_path.parent().unwrap().join(header_file_name);
        let header_file = open_file_sync(&header_file_path.into())?;
        let reader = io::BufReader::new(header_file);
        let detector_xml: DetectorXml =
            de::from_reader(reader).expect("Failed to parse the header file");
        create_or_update_header(connection, &detector_xml, system_id).await;
    }

    // persist games
    progress_bar.println("Deleting old games");
    delete_old_games(connection, &datafile_xml.games, system_id).await;
    progress_bar.println("Processing games");
    create_or_update_games(connection, &datafile_xml.games, system_id, &progress_bar).await?;

    Ok(())
}

fn get_regions_from_game_name<'a>(name: &str) -> SimpleResult<String> {
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

async fn create_or_update_system(connection: &mut SqliteConnection, system_xml: &SystemXml) -> i64 {
    let system = find_system_by_name(connection, &system_xml.name).await;
    match system {
        Some(system) => {
            update_system(connection, system.id, system_xml).await;
            system.id
        }
        None => create_system(connection, system_xml).await,
    }
}

async fn create_or_update_header(
    connection: &mut SqliteConnection,
    detector_xml: &DetectorXml,
    system_id: i64,
) {
    let header = find_header_by_system_id(connection, system_id).await;
    match header {
        Some(header) => {
            update_header(connection, header.id, detector_xml, system_id).await;
            header.id
        }
        None => create_header(connection, detector_xml, system_id).await,
    };
}

async fn create_or_update_games(
    connection: &mut SqliteConnection,
    games_xml: &[GameXml],
    system_id: i64,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let mut orphan_romfile_ids: Vec<i64> = Vec::new();
    let parent_games_xml: Vec<&GameXml> = games_xml
        .iter()
        .filter(|game_xml| game_xml.cloneof.is_none())
        .collect();
    let child_games_xml: Vec<&GameXml> = games_xml
        .iter()
        .filter(|game_xml| game_xml.cloneof.is_some())
        .collect();
    for parent_game_xml in parent_games_xml {
        let game =
            find_game_by_name_and_system_id(connection, &parent_game_xml.name, system_id).await;
        let game_id = match game {
            Some(game) => {
                update_game(
                    connection,
                    game.id,
                    parent_game_xml,
                    &get_regions_from_game_name(&parent_game_xml.name).unwrap(),
                    system_id,
                    None,
                )
                .await;
                game.id
            }
            None => {
                create_game(
                    connection,
                    parent_game_xml,
                    &get_regions_from_game_name(&parent_game_xml.name).unwrap(),
                    system_id,
                    None,
                )
                .await
            }
        };
        if !parent_game_xml.releases.is_empty() {
            create_or_update_releases(connection, &parent_game_xml.releases, game_id).await;
        }
        delete_old_releases(connection, &parent_game_xml.releases, game_id).await;
        if !parent_game_xml.roms.is_empty() {
            create_or_update_roms(connection, &parent_game_xml.roms, game_id).await;
        }
        orphan_romfile_ids
            .append(&mut delete_old_roms(connection, &parent_game_xml.roms, game_id).await);
        progress_bar.inc(1)
    }
    for child_game_xml in child_games_xml {
        let game =
            find_game_by_name_and_system_id(connection, &child_game_xml.name, system_id).await;
        let parent_game = find_game_by_name_and_system_id(
            connection,
            child_game_xml.cloneof.as_ref().unwrap(),
            system_id,
        )
        .await
        .unwrap();
        let game_id = match game {
            Some(game) => {
                update_game(
                    connection,
                    game.id,
                    child_game_xml,
                    &get_regions_from_game_name(&child_game_xml.name).unwrap(),
                    system_id,
                    Some(parent_game.id),
                )
                .await;
                game.id
            }
            None => {
                create_game(
                    connection,
                    child_game_xml,
                    &get_regions_from_game_name(&child_game_xml.name).unwrap(),
                    system_id,
                    Some(parent_game.id),
                )
                .await
            }
        };
        if !child_game_xml.releases.is_empty() {
            create_or_update_releases(connection, &child_game_xml.releases, game_id).await;
        }
        delete_old_releases(connection, &child_game_xml.releases, game_id).await;
        if !child_game_xml.roms.is_empty() {
            create_or_update_roms(connection, &child_game_xml.roms, game_id).await;
        }
        orphan_romfile_ids
            .append(&mut delete_old_roms(connection, &child_game_xml.roms, game_id).await);
        progress_bar.inc(1)
    }
    if !orphan_romfile_ids.is_empty() {
        progress_bar.println("Reimporting orphan romfiles");
        reimport_orphan_romfiles(connection, system_id, orphan_romfile_ids, progress_bar).await?;
    }
    Ok(())
}

async fn create_or_update_releases(
    connection: &mut SqliteConnection,
    releases_xml: &[ReleaseXml],
    game_id: i64,
) {
    for release_xml in releases_xml {
        let release = find_release_by_name_and_region_and_game_id(
            connection,
            &release_xml.name,
            &release_xml.region,
            game_id,
        )
        .await;
        match release {
            Some(release) => {
                update_release(connection, release.id, &release_xml, game_id).await;
                release.id
            }
            None => create_release(connection, &release_xml, game_id).await,
        };
    }
}

async fn create_or_update_roms(
    connection: &mut SqliteConnection,
    roms_xml: &[RomXml],
    game_id: i64,
) {
    for rom_xml in roms_xml {
        let rom = find_rom_by_name_and_game_id(connection, &rom_xml.name, game_id).await;
        match rom {
            Some(rom) => {
                update_rom(connection, rom.id, &rom_xml, game_id).await;
                rom.id
            }
            None => create_rom(connection, &rom_xml, game_id).await,
        };
    }
}

async fn delete_old_games(
    connection: &mut SqliteConnection,
    games_xml: &[GameXml],
    system_id: i64,
) {
    let game_names_xml: Vec<&String> = games_xml.iter().map(|game_xml| &game_xml.name).collect();
    let game_names: Vec<String> = find_games_by_system_id(connection, system_id)
        .await
        .into_par_iter()
        .map(|game| game.name)
        .collect();
    for game_name in &game_names {
        if !game_names_xml.contains(&game_name) {
            delete_game_by_name_and_system_id(connection, &game_name, system_id).await
        }
    }
}

async fn delete_old_releases(
    connection: &mut SqliteConnection,
    releases_xml: &[ReleaseXml],
    game_id: i64,
) {
    let release_names_regions_xml: Vec<(&String, &String)> = releases_xml
        .iter()
        .map(|release_xml| (&release_xml.name, &release_xml.region))
        .collect();
    let release_names_regions: Vec<(String, String)> =
        find_releases_by_game_id(connection, game_id)
            .await
            .into_par_iter()
            .map(|release| (release.name, release.region))
            .collect();
    for release_tuple in &release_names_regions {
        if !release_names_regions_xml.contains(&(&release_tuple.0, &release_tuple.1)) {
            delete_release_by_name_and_region_and_game_id(
                connection,
                &release_tuple.0,
                &release_tuple.1,
                game_id,
            )
            .await
        }
    }
}

async fn delete_old_roms(
    connection: &mut SqliteConnection,
    roms_xml: &[RomXml],
    game_id: i64,
) -> Vec<i64> {
    let rom_names_xml: Vec<&String> = roms_xml.iter().map(|rom_xml| &rom_xml.name).collect();
    let rom_names_romfile_ids: Vec<(String, Option<i64>)> =
        find_roms_by_game_id(connection, game_id)
            .await
            .into_par_iter()
            .map(|rom| (rom.name, rom.romfile_id))
            .collect();
    let mut orphan_romfile_ids: Vec<i64> = Vec::new();
    for (rom_name, rom_romfile_id) in &rom_names_romfile_ids {
        if !rom_names_xml.contains(&rom_name) {
            delete_rom_by_name_and_game_id(connection, &rom_name, game_id).await;
            if let Some(romfile_id) = rom_romfile_id {
                orphan_romfile_ids.push(*romfile_id);
            }
        }
    }
    orphan_romfile_ids
}

async fn reimport_orphan_romfiles(
    connection: &mut SqliteConnection,
    system_id: i64,
    orphan_romfile_ids: Vec<i64>,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let system = find_system_by_id(connection, system_id).await;
    let header = find_header_by_system_id(connection, system_id).await;
    let system_directory = get_rom_directory(connection).await.join(&system.name);
    create_directory(&system_directory).await?;
    for romfile_id in orphan_romfile_ids {
        let romfile = find_romfile_by_id(connection, romfile_id).await;
        delete_romfile_by_id(connection, romfile_id).await;
        import_rom(
            connection,
            &system_directory,
            &system,
            &header,
            &Path::new(&romfile.path).to_path_buf(),
            progress_bar,
        )
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use super::super::database::*;
    use super::*;
    use async_std::fs;
    use async_std::path::Path;
    use async_std::sync::Mutex;
    use tempfile::{NamedTempFile, TempDir};

    #[async_std::test]
    async fn test_import_dat() {
        // given
        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let dat_path = test_directory.join("Test System 20200721.dat");

        // when
        import_dat(&mut connection, &dat_path, false, &progress_bar)
            .await
            .unwrap();

        // then
        let mut systems = find_systems(&mut connection).await;
        assert_eq!(systems.len(), 1);

        let system = systems.remove(0);
        assert_eq!(system.name, "Test System");

        assert_eq!(find_games(&mut connection).await.len(), 6);
        assert_eq!(find_releases(&mut connection).await.len(), 10);
        assert_eq!(find_roms(&mut connection).await.len(), 8);
    }

    #[async_std::test]
    async fn test_import_dat_parent_clone() {
        // given
        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let dat_path = test_directory.join("Test System 20200721 (Parent-Clone).dat");

        // when
        import_dat(&mut connection, &dat_path, false, &progress_bar)
            .await
            .unwrap();

        // then
        let mut systems = find_systems(&mut connection).await;
        assert_eq!(systems.len(), 1);

        let system = systems.remove(0);
        assert_eq!(system.name, "Test System");

        assert_eq!(find_games(&mut connection).await.len(), 4);
        assert_eq!(find_releases(&mut connection).await.len(), 6);
        assert_eq!(find_roms(&mut connection).await.len(), 4);
    }

    #[async_std::test]
    async fn test_import_dat_info() {
        // given
        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let dat_path = test_directory.join("Test System 20200721.dat");

        // when
        import_dat(&mut connection, &dat_path, true, &progress_bar)
            .await
            .unwrap();

        // then
        let systems = find_systems(&mut connection).await;
        assert_eq!(systems.len(), 0);

        assert_eq!(find_games(&mut connection).await.len(), 0);
        assert_eq!(find_releases(&mut connection).await.len(), 0);
        assert_eq!(find_roms(&mut connection).await.len(), 0);
    }

    #[async_std::test]
    async fn test_import_updated_dat() {
        // given
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let dat_path = test_directory.join("Test System 20200721.dat");
        import_dat(&mut connection, &dat_path, false, &progress_bar)
            .await
            .unwrap();

        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        set_rom_directory(PathBuf::from(&tmp_directory.path()));
        let tmp_path = set_tmp_directory(PathBuf::from(&tmp_directory.path()));
        let system_path = &tmp_path.join("Test System");
        create_directory(&system_path).await.unwrap();
        let romfile_path = tmp_path.join("Test Game (USA, Europe).rom");
        fs::copy(
            test_directory.join("Test Game (USA, Europe).rom"),
            &romfile_path.as_os_str().to_str().unwrap(),
        )
        .await
        .unwrap();

        let system = find_systems(&mut connection).await.remove(0);

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

        let dat_path = test_directory.join("Test System 20210401.dat");

        // when
        import_dat(&mut connection, &dat_path, false, &progress_bar)
            .await
            .unwrap();

        // then
        let mut systems = find_systems(&mut connection).await;
        assert_eq!(systems.len(), 1);

        let system = systems.remove(0);
        assert_eq!(system.name, "Test System");

        let mut games = find_games(&mut connection).await;
        let mut releases = find_releases(&mut connection).await;
        let mut roms = find_roms(&mut connection).await;
        let mut romfiles = find_romfiles(&mut connection).await;

        assert_eq!(games.len(), 1);
        assert_eq!(releases.len(), 1);
        assert_eq!(roms.len(), 1);
        assert_eq!(romfiles.len(), 1);

        let game = games.remove(0);
        let release = releases.remove(0);
        let rom = roms.remove(0);
        let romfile = romfiles.remove(0);

        assert_eq!(game.name, "Test Game (USA, Europe)");
        assert_eq!(release.name, "Test Game (USA, Europe)");
        assert_eq!(release.region, "USA");
        assert_eq!(rom.name, "Test Game (USA, Europe).bin");
        assert!(rom.romfile_id.is_some());
        assert_eq!(rom.romfile_id.unwrap(), romfile.id);
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
}

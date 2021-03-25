use super::database::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use super::SimpleResult;
use async_std::path::PathBuf;
use clap::{App, Arg, ArgMatches, SubCommand};
use indicatif::ProgressBar;
use once_cell::sync::OnceCell;
use quick_xml::de;
use rayon::prelude::*;
use regex::Regex;
use sqlx::SqliteConnection;
use std::io;

static REGION_CODES: OnceCell<Vec<(Regex, Vec<&'static str>)>> = OnceCell::new();

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

    // persist everything into the database
    progress_bar.println("Processing system");
    let system_id = create_or_update_system(connection, &datafile_xml.system).await;
    progress_bar.println("Deleting old games");
    delete_old_games(connection, &datafile_xml.games, system_id).await;
    progress_bar.println("Processing games");
    create_or_update_games(connection, &datafile_xml.games, system_id, &progress_bar).await;

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

    Ok(())
}

fn get_regions_from_game_name<'a>(name: &str) -> Vec<&'a str> {
    let region_codes = REGION_CODES.get_or_init(|| {
        vec![
            (Regex::new(r"\(.*Asia,?.*\)").unwrap(), vec!["ASI"]),
            (Regex::new(r"\(.*Australia,?.*\)").unwrap(), vec!["AUS"]),
            (Regex::new(r"\(.*Austria,?.*\)").unwrap(), vec!["AUT"]),
            (Regex::new(r"\(.*Belgium,?.*\)").unwrap(), vec!["BEL"]),
            (Regex::new(r"\(.*Brazil,?.*\)").unwrap(), vec!["BRA"]),
            (Regex::new(r"\(.*Canada,?.*\)").unwrap(), vec!["CAN"]),
            (Regex::new(r"\(.*China,?.*\)").unwrap(), vec!["CHN"]),
            (Regex::new(r"\(.*Denmark,?.*\)").unwrap(), vec!["DAN"]),
            (Regex::new(r"\(.*Europe,?.*\)").unwrap(), vec!["EUR"]),
            (Regex::new(r"\(.*Finland,?.*\)").unwrap(), vec!["FIN"]),
            (Regex::new(r"\(.*France,?.*\)").unwrap(), vec!["FRA"]),
            (Regex::new(r"\(.*Germany,?.*\)").unwrap(), vec!["GER"]),
            (Regex::new(r"\(.*Greece,?.*\)").unwrap(), vec!["GRC"]),
            (Regex::new(r"\(.*Hong Kong,?.*\)").unwrap(), vec!["HKG"]),
            (Regex::new(r"\(.*Ireland,?.*\)").unwrap(), vec!["IRL"]),
            (Regex::new(r"\(.*Israel,?.*\)").unwrap(), vec!["ISR"]),
            (Regex::new(r"\(.*Italy,?.*\)").unwrap(), vec!["ITA"]),
            (Regex::new(r"\(.*Japan,?.*\)").unwrap(), vec!["JPN"]),
            (Regex::new(r"\(.*Korea,?.*\)").unwrap(), vec!["KOR"]),
            (Regex::new(r"\(.*Netherlands,?.*\)").unwrap(), vec!["HOL"]),
            (Regex::new(r"\(.*Norway,?.*\)").unwrap(), vec!["NOR"]),
            (Regex::new(r"\(.*Poland,?.*\)").unwrap(), vec!["POL"]),
            (Regex::new(r"\(.*Portugal,?.*\)").unwrap(), vec!["PRT"]),
            (Regex::new(r"\(.*Russia,?.*\)").unwrap(), vec!["RUS"]),
            (Regex::new(r"\(.*Scandinavia,?.*\)").unwrap(), vec!["SCA"]),
            (Regex::new(r"\(.*Spain,?.*\)").unwrap(), vec!["SPA"]),
            (Regex::new(r"\(.*Sweden,?.*\)").unwrap(), vec!["SWE"]),
            (Regex::new(r"\(.*Taiwan,?.*\)").unwrap(), vec!["TAI"]),
            (Regex::new(r"\(.*UK,?.*\)").unwrap(), vec!["GBR"]),
            (Regex::new(r"\(.*Unknown,?.*\)").unwrap(), vec!["UNK"]),
            (Regex::new(r"\(.*USA,?.*\)").unwrap(), vec!["USA"]),
            (
                Regex::new(r"\(.*World,?.*\)").unwrap(),
                vec!["EUR", "JPN", "USA"],
            ),
        ]
    });
    let mut regions: Vec<&str> = Vec::new();
    for (re, regions_vec) in region_codes {
        if re.find(name).is_some() {
            regions.append(&mut regions_vec.clone());
        }
    }
    regions.sort_unstable();
    regions.dedup();
    regions
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

async fn create_or_update_games(
    connection: &mut SqliteConnection,
    games_xml: &[GameXml],
    system_id: i64,
    progress_bar: &ProgressBar,
) {
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
                    &game.regions,
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
                    &get_regions_from_game_name(&parent_game_xml.name).join(","),
                    system_id,
                    None,
                )
                .await
            }
        };
        if !parent_game_xml.releases.is_empty() {
            create_or_update_releases(connection, &parent_game_xml.releases, game_id).await;
        }
        if !parent_game_xml.roms.is_empty() {
            create_or_update_roms(connection, &parent_game_xml.roms, game_id).await;
        }
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
                    &game.regions,
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
                    &get_regions_from_game_name(&child_game_xml.name).join(","),
                    system_id,
                    Some(parent_game.id),
                )
                .await
            }
        };
        if !child_game_xml.releases.is_empty() {
            create_or_update_releases(connection, &child_game_xml.releases, game_id).await;
        }
        if !child_game_xml.roms.is_empty() {
            create_or_update_roms(connection, &child_game_xml.roms, game_id).await;
        }
        progress_bar.inc(1)
    }
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

#[cfg(test)]
mod test {
    use super::super::database::*;
    use super::*;
    use async_std::path::Path;
    use tempfile::NamedTempFile;

    #[async_std::test]
    async fn test_import_dat() {
        // given
        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

        let dat_path = test_directory.join("Test System.dat");

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

        let dat_path = test_directory.join("Test System (Parent-Clone).dat");

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

        let dat_path = test_directory.join("Test System.dat");

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

    #[test]
    fn test_get_regions_from_game_name_world() {
        // given
        let game_name = "Test Game (World)";

        // when
        let mut regions = get_regions_from_game_name(game_name);

        // then
        assert_eq!(regions.len(), 3);
        assert_eq!(regions.remove(0), "EUR");
        assert_eq!(regions.remove(0), "JPN");
        assert_eq!(regions.remove(0), "USA");
    }

    #[test]
    fn test_get_regions_from_game_name_france_germany() {
        // given
        let game_name = "Test Game (France, Germany)";

        // when
        let mut regions = get_regions_from_game_name(game_name);

        // then
        assert_eq!(regions.len(), 2);
        assert_eq!(regions.remove(0), "FRA");
        assert_eq!(regions.remove(0), "GER");
    }
}

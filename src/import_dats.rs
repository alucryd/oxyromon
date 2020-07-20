use super::crud::*;
use super::model::*;
use super::progress::*;
use super::util::*;
use super::SimpleResult;
use clap::{App, Arg, ArgMatches, SubCommand};
use diesel::SqliteConnection;
use indicatif::ProgressBar;
use quick_xml::de;
use regex::Regex;
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

pub fn main(connection: &SqliteConnection, matches: &ArgMatches) -> SimpleResult<()> {
    let region_codes: Vec<(Regex, Vec<&str>)> = vec![
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
    ];

    for d in matches.values_of("DATS").unwrap() {
        let dat_path = get_canonicalized_path(d)?;
        let f = open_file(&dat_path)?;
        let reader = io::BufReader::new(f);
        let mut datafile_xml: DatfileXml =
            de::from_reader(reader).expect("Failed to parse the datafile");

        // strip the parentheses qualifiers from the system name
        let re = Regex::new(r"\(.*\)").unwrap();
        datafile_xml.system.name = re.replace(&datafile_xml.system.name, "").trim().to_owned();

        // print information
        println!("System: {}", datafile_xml.system.name);
        println!("Version: {}", datafile_xml.system.version);
        println!("Games: {}", datafile_xml.games.len());
        if matches.is_present("INFO") {
            continue;
        }

        let progress_bar =
            get_progress_bar(datafile_xml.games.len() as u64, get_count_progress_style());

        // persist everything into the database
        progress_bar.set_message("Processing system");
        let system_id = create_or_update_system(&connection, &datafile_xml.system);
        progress_bar.set_message("Deleting old games");
        delete_old_games(&connection, &datafile_xml.games, system_id);
        progress_bar.set_message("Processing games");
        create_or_update_games(
            &connection,
            &datafile_xml.games,
            system_id,
            &region_codes,
            &progress_bar,
        );

        // parse header file if needed
        if datafile_xml.system.clrmamepro.is_some() {
            progress_bar.set_message("Processing header");
            let header_file_name = &datafile_xml.system.clrmamepro.unwrap().header;
            let header_file_path = dat_path.parent().unwrap().join(header_file_name);
            let header_file = open_file(&header_file_path)?;
            let reader = io::BufReader::new(header_file);
            let detector_xml: DetectorXml =
                de::from_reader(reader).expect("Failed to parse the header file");
            create_or_update_header(&connection, &detector_xml, system_id);
        }
    }

    Ok(())
}

fn get_regions_from_game_name<'r>(
    name: &String,
    region_codes: &Vec<(Regex, Vec<&'r str>)>,
) -> Vec<&'r str> {
    let mut regions: Vec<&str> = Vec::new();
    for (re, regions_vec) in region_codes {
        if re.find(name).is_some() {
            regions.append(&mut regions_vec.clone());
        }
    }
    regions.sort();
    regions.dedup();
    return regions;
}

fn create_or_update_system(connection: &SqliteConnection, system_xml: &SystemXml) -> i64 {
    let system = find_system_by_name(connection, &system_xml.name);
    match system {
        Some(system) => {
            update_system(connection, &system, system_xml);
            system.id
        }
        None => create_system(connection, system_xml),
    }
}

fn create_or_update_header(
    connection: &SqliteConnection,
    detector_xml: &DetectorXml,
    system_id: i64,
) {
    let header = find_header_by_system_id(connection, system_id);
    match header {
        Some(header) => {
            update_header(connection, &header, detector_xml, system_id);
            header.id
        }
        None => create_header(connection, detector_xml, system_id),
    };
}

fn delete_old_games(connection: &SqliteConnection, games_xml: &Vec<GameXml>, system_id: i64) {
    let game_names_xml: Vec<&String> = games_xml.iter().map(|game_xml| &game_xml.name).collect();
    let game_names = find_game_names_by_system_id(&connection, system_id);
    for game_name in game_names {
        if !game_names_xml.contains(&&game_name) {
            delete_game_by_name_and_system_id(&connection, &game_name, system_id)
        }
    }
}

fn create_or_update_games(
    connection: &SqliteConnection,
    games_xml: &Vec<GameXml>,
    system_id: i64,
    region_codes: &Vec<(Regex, Vec<&str>)>,
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
        let game = find_game_by_name_and_system_id(connection, &parent_game_xml.name, system_id);
        let game_id = match game {
            Some(game) => {
                update_game(
                    connection,
                    &game,
                    parent_game_xml,
                    &game.regions,
                    system_id,
                    None,
                );
                game.id
            }
            None => create_game(
                connection,
                parent_game_xml,
                &get_regions_from_game_name(&parent_game_xml.name, region_codes).join(","),
                system_id,
                None,
            ),
        };
        if !parent_game_xml.releases.is_empty() {
            create_or_update_releases(connection, &parent_game_xml.releases, game_id);
        }
        if !parent_game_xml.roms.is_empty() {
            create_or_update_roms(connection, &parent_game_xml.roms, game_id);
        }
        progress_bar.inc(1)
    }
    for child_game_xml in child_games_xml {
        let game = find_game_by_name_and_system_id(connection, &child_game_xml.name, system_id);
        let parent_game = find_game_by_name_and_system_id(
            connection,
            child_game_xml.cloneof.as_ref().unwrap(),
            system_id,
        )
        .unwrap();
        let game_id = match game {
            Some(game) => {
                update_game(
                    connection,
                    &game,
                    child_game_xml,
                    &game.regions,
                    system_id,
                    Some(parent_game.id),
                );
                game.id
            }
            None => create_game(
                connection,
                child_game_xml,
                &get_regions_from_game_name(&child_game_xml.name, region_codes).join(","),
                system_id,
                Some(parent_game.id),
            ),
        };
        if !child_game_xml.releases.is_empty() {
            create_or_update_releases(connection, &child_game_xml.releases, game_id);
        }
        if !child_game_xml.roms.is_empty() {
            create_or_update_roms(connection, &child_game_xml.roms, game_id);
        }
        progress_bar.inc(1)
    }
}

fn create_or_update_releases(
    connection: &SqliteConnection,
    releases_xml: &Vec<ReleaseXml>,
    game_id: i64,
) {
    for release_xml in releases_xml {
        let release = find_release_by_name_and_region_and_game_id(
            connection,
            &release_xml.name,
            &release_xml.region,
            game_id,
        );
        match release {
            Some(release) => {
                update_release(connection, &release, &release_xml, game_id);
                release.id
            }
            None => create_release(connection, &release_xml, game_id),
        };
    }
}

fn create_or_update_roms(connection: &SqliteConnection, roms_xml: &Vec<RomXml>, game_id: i64) {
    for rom_xml in roms_xml {
        let rom = find_rom_by_name_and_game_id(connection, &rom_xml.name, game_id);
        match rom {
            Some(rom) => {
                update_rom(connection, &rom, &rom_xml, game_id);
                rom.id
            }
            None => create_rom(connection, &rom_xml, game_id),
        };
    }
}

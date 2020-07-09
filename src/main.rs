extern crate clap;
extern crate crc32fast;
#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;
extern crate digest;
extern crate dirs;
extern crate dotenv;
extern crate quick_xml;
extern crate regex;
extern crate serde;
#[macro_use]
extern crate simple_error;
extern crate rayon;

mod chdman;
mod checksum;
mod config;
mod convert_roms;
mod crud;
mod import_dats;
mod import_roms;
mod maxcso;
mod model;
mod prompt;
mod purge_roms;
mod schema;
mod sevenzip;
mod sort_roms;
mod util;

use self::config::config;
use self::convert_roms::convert_roms;
use self::crud::*;
use self::import_dats::import_dats;
use self::import_roms::import_roms;
use self::purge_roms::purge_roms;
use self::sort_roms::sort_roms;
use self::util::*;
use clap::{App, Arg, SubCommand};
use diesel::prelude::*;
use diesel::SqliteConnection;
use dotenv::dotenv;
use simple_error::SimpleError;
use std::env;
use std::path::PathBuf;

embed_migrations!("migrations");

pub type SimpleResult<T> = Result<T, SimpleError>;

fn main() -> SimpleResult<()> {
    let config_subcommand: App = SubCommand::with_name("config")
        .about("Queries and modifies the oxyromon settings")
        .arg(
            Arg::with_name("LIST")
                .short("l")
                .long("list")
                .help("Prints the whole configuration")
                .required(false)
                .conflicts_with_all(&["GET", "SET"]),
        )
        .arg(
            Arg::with_name("GET")
                .short("g")
                .long("get")
                .help("Prints a single setting")
                .required(false)
                .takes_value(true)
                .value_name("KEY"),
        )
        .arg(
            Arg::with_name("SET")
                .short("s")
                .long("set")
                .help("Configures a single setting")
                .required(false)
                .takes_value(true)
                .multiple(true)
                .number_of_values(2)
                .value_names(&["KEY", "VALUE"]),
        )
        .arg(
            Arg::with_name("DELETE")
                .short("d")
                .long("delete")
                .help("Deletes a single setting")
                .required(false)
                .takes_value(true)
                .value_name("KEY"),
        );

    let import_dats_subcommand: App = SubCommand::with_name("import-dats")
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
        );

    let import_roms_subcommand: App = SubCommand::with_name("import-roms")
        .about("Validates and imports ROM files into oxyromon")
        .arg(
            Arg::with_name("ROMS")
                .help("Sets the ROM files to import")
                .required(true)
                .multiple(true)
                .index(1),
        );

    let sort_roms_subcommand: App = SubCommand::with_name("sort-roms")
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
            Arg::with_name("NO-BETA")
                .long("no-beta")
                .help("Discards beta games")
                .required(false),
        )
        .arg(
            Arg::with_name("NO-DEBUG")
                .long("no-debug")
                .help("Discards debug games")
                .required(false),
        )
        .arg(
            Arg::with_name("NO-DEMO")
                .long("no-demo")
                .help("Discards demo games")
                .required(false),
        )
        .arg(
            Arg::with_name("NO-PROGRAM")
                .long("no-program")
                .help("Discards program games")
                .required(false),
        )
        .arg(
            Arg::with_name("NO-PROTO")
                .long("no-proto")
                .help("Discards prototype games")
                .required(false),
        )
        .arg(
            Arg::with_name("NO-SAMPLE")
                .long("no-sample")
                .help("Discards sample games")
                .required(false),
        )
        .arg(
            Arg::with_name("NO-SEGA-CHANNEL")
                .long("no-sega-channel")
                .help("Discards sega channel games")
                .required(false),
        )
        .arg(
            Arg::with_name("NO-VIRTUAL-CONSOLE")
                .long("no-virtual-console")
                .help("Discards virtual console games")
                .required(false),
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
        );

    let convert_roms_subcommand: App = SubCommand::with_name("convert-roms")
        .about("Converts ROM files between common formats")
        .arg(
            Arg::with_name("FORMAT")
                .short("f")
                .long("format")
                .help("Sets the destination format")
                .required(false)
                .takes_value(true)
                .possible_values(&["7Z", "CHD", "CSO", "ORIGINAL", "ZIP"]),
        );

    let purge_roms_subcommand: App = SubCommand::with_name("purge-roms")
        .about("Purges trashed and missing ROM files")
        .arg(
            Arg::with_name("EMPTY-TRASH")
                .short("t")
                .long("empty-trash")
                .help("Empties the ROM files trash directories")
                .required(false),
        )
        .arg(
            Arg::with_name("YES")
                .short("y")
                .long("yes")
                .help("Automatically says yes to prompts")
                .required(false),
        );

    let matches = App::new("oxyromon")
        .version("0.1.0")
        .about("Rusty ROM OrgaNizer")
        .author("Maxime Gauduin <alucryd@archlinux.org>")
        .subcommands(vec![
            config_subcommand,
            import_dats_subcommand,
            import_roms_subcommand,
            sort_roms_subcommand,
            convert_roms_subcommand,
            purge_roms_subcommand,
        ])
        .get_matches();

    if matches.subcommand.is_some() {
        dotenv().ok();
        let data_directory = dirs::data_dir().unwrap().join("oxyromon");
        create_directory(&data_directory)?;
        let connection = establish_connection(&data_directory)?;
        let rom_directory = get_directory(
            &connection,
            "ROM_DIRECTORY",
            dirs::home_dir().unwrap().join("Emulation"),
        )?;
        create_directory(&rom_directory)?;
        let tmp_directory = get_directory(&connection, "TMP_DIRECTORY", env::temp_dir())?;
        create_directory(&tmp_directory)?;

        match matches.subcommand_name() {
            Some("config") => config(&connection, &matches.subcommand_matches("config").unwrap())?,
            Some("import-dats") => import_dats(
                &connection,
                &matches.subcommand_matches("import-dats").unwrap(),
            )?,
            Some("import-roms") => import_roms(
                &connection,
                &matches.subcommand_matches("import-roms").unwrap(),
                &rom_directory,
                &tmp_directory,
            )?,
            Some("sort-roms") => sort_roms(
                &connection,
                &matches.subcommand_matches("sort-roms").unwrap(),
                &rom_directory,
            )?,
            Some("convert-roms") => convert_roms(
                &connection,
                &matches.subcommand_matches("convert-roms").unwrap(),
                &tmp_directory,
            )?,
            Some("purge-roms") => purge_roms(
                &connection,
                &matches.subcommand_matches("purge-roms").unwrap(),
            )?,
            _ => (),
        }

        close_connection(connection)?;
    }

    Ok(())
}

fn establish_connection(directory: &PathBuf) -> SimpleResult<SqliteConnection> {
    let database_path = directory.join("oxyromon.db");
    let database_url = database_path.as_os_str().to_str().unwrap();
    let connection = SqliteConnection::establish(&database_url)
        .expect(&format!("Error connecting to {}", database_url));

    try_with!(
        connection.execute(
            "
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = WAL;
            PRAGMA locking_mode = EXCLUSIVE;
            PRAGMA synchronous = NORMAL;
            PRAGMA temp_store = MEMORY;
            PRAGMA wal_checkpoint(TRUNCATE);
            ",
        ),
        "Failed to setup the database"
    );

    try_with!(
        embedded_migrations::run(&connection),
        "Failed to run embedded migrations"
    );

    Ok(connection)
}

fn close_connection(connection: SqliteConnection) -> SimpleResult<()> {
    try_with!(
        connection.execute("PRAGMA optimize;"),
        "Failed to optimize the database"
    );
    Ok(())
}

fn get_directory(
    connection: &SqliteConnection,
    key: &str,
    default: PathBuf,
) -> SimpleResult<PathBuf> {
    let directory = match find_setting_by_key(&connection, key) {
        Some(setting) => match setting.value {
            Some(directory) => get_canonicalized_path(&directory)?,
            None => {
                update_setting(connection, &setting, default.as_os_str().to_str().unwrap());
                default
            }
        },
        None => {
            create_setting(connection, key, default.as_os_str().to_str().unwrap());
            default
        }
    };
    Ok(directory)
}

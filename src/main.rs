extern crate clap;
extern crate crc32fast;
#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;
extern crate digest;
extern crate dirs;
extern crate dotenv;
extern crate indicatif;
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
mod progress;
mod prompt;
mod purge_roms;
mod schema;
mod sevenzip;
mod sort_roms;
mod util;

use self::config::*;
use self::convert_roms::convert_roms;
use self::import_dats::import_dats;
use self::import_roms::import_roms;
use self::purge_roms::purge_roms;
use self::util::*;
use clap::{App, Arg, SubCommand};
use diesel::prelude::*;
use diesel::SqliteConnection;
use dotenv::dotenv;
use simple_error::SimpleError;

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
            Arg::with_name("EMPTY_TRASH")
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
            sort_roms::subcommand(),
            convert_roms_subcommand,
            purge_roms_subcommand,
        ])
        .get_matches();

    if matches.subcommand.is_some() {
        dotenv().ok();
        let data_directory = dirs::data_dir().unwrap().join("oxyromon");
        create_directory(&data_directory)?;
        let connection = establish_connection(
            data_directory
                .join("oxyromon.db")
                .as_os_str()
                .to_str()
                .unwrap(),
        )?;
        let rom_directory = get_rom_directory(&connection);
        create_directory(&rom_directory)?;
        let tmp_directory = get_tmp_directory(&connection);
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
            Some("sort-roms") => sort_roms::main(
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

fn establish_connection(url: &str) -> SimpleResult<SqliteConnection> {
    let connection =
        SqliteConnection::establish(url).expect(&format!("Error connecting to {}", url));

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

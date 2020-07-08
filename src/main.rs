extern crate clap;
extern crate crc32fast;
#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;
extern crate digest;
extern crate dotenv;
extern crate quick_xml;
extern crate regex;
extern crate serde;
#[macro_use]
extern crate simple_error;
extern crate rayon;

mod chdman;
mod checksum;
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

use self::convert_roms::convert_roms;
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
            import_dats_subcommand,
            import_roms_subcommand,
            sort_roms_subcommand,
            convert_roms_subcommand,
            purge_roms_subcommand,
        ])
        .get_matches();

    if matches.subcommand.is_some() {
        dotenv().ok();
        let rom_directory =
            get_canonicalized_path(&env::var("ROM_DIRECTORY").expect("ROM_DIRECTORY must be set"))?;
        let tmp_directory = env::var("TMP_DIRECTORY");
        let tmp_directory = match tmp_directory {
            Ok(tmp_directory) => get_canonicalized_path(&tmp_directory)?,
            Err(_) => env::temp_dir(),
        };
        let connection = establish_connection(&rom_directory)?;

        match matches.subcommand_name() {
            Some("convert-roms") => convert_roms(
                &connection,
                &matches.subcommand_matches("convert-roms").unwrap(),
                &tmp_directory,
            )?,
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
            Some("purge-roms") => purge_roms(
                &connection,
                &matches.subcommand_matches("purge-roms").unwrap(),
            )?,
            Some("sort-roms") => sort_roms(
                &connection,
                &matches.subcommand_matches("sort-roms").unwrap(),
                &rom_directory,
            )?,
            _ => (),
        }

        close_connection(connection)?;
    }

    Ok(())
}

fn establish_connection(rom_directory: &PathBuf) -> SimpleResult<SqliteConnection> {
    let database_path = rom_directory.join(".oxyromon.db");
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

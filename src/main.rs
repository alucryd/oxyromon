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
use self::util::*;
use clap::App;
use diesel::prelude::*;
use diesel::SqliteConnection;
use dotenv::dotenv;
use simple_error::SimpleError;

embed_migrations!("migrations");

pub type SimpleResult<T> = Result<T, SimpleError>;

fn main() -> SimpleResult<()> {
    let matches = App::new("oxyromon")
        .version("0.1.0")
        .about("Rusty ROM OrgaNizer")
        .author("Maxime Gauduin <alucryd@archlinux.org>")
        .subcommands(vec![
            config::subcommand(),
            import_dats::subcommand(),
            import_roms::subcommand(),
            sort_roms::subcommand(),
            convert_roms::subcommand(),
            purge_roms::subcommand(),
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
            Some("config") => {
                config::main(&connection, &matches.subcommand_matches("config").unwrap())?
            }
            Some("import-dats") => import_dats::main(
                &connection,
                &matches.subcommand_matches("import-dats").unwrap(),
            )?,
            Some("import-roms") => import_roms::main(
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
            Some("convert-roms") => convert_roms::main(
                &connection,
                &matches.subcommand_matches("convert-roms").unwrap(),
                &tmp_directory,
            )?,
            Some("purge-roms") => purge_roms::main(
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

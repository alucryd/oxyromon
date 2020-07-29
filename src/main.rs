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
extern crate once_cell;
extern crate quick_xml;
extern crate regex;
extern crate serde;
#[macro_use]
extern crate simple_error;
extern crate rayon;
extern crate tempfile;

mod chdman;
mod checksum;
mod config;
mod convert_roms;
mod database;
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

use clap::App;
use database::*;
use dotenv::dotenv;
use simple_error::SimpleError;
use util::*;

type SimpleResult<T> = Result<T, SimpleError>;

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
            )?,
            Some("sort-roms") => sort_roms::main(
                &connection,
                &matches.subcommand_matches("sort-roms").unwrap(),
            )?,
            Some("convert-roms") => convert_roms::main(
                &connection,
                &matches.subcommand_matches("convert-roms").unwrap(),
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

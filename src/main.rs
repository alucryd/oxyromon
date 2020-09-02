extern crate async_std;
extern crate clap;
extern crate crc32fast;
extern crate digest;
extern crate dirs;
extern crate dotenv;
extern crate indicatif;
extern crate once_cell;
extern crate quick_xml;
extern crate refinery;
extern crate regex;
extern crate serde;
extern crate sqlx;
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
mod sevenzip;
mod sort_roms;
mod util;

mod embedded {
    use refinery::embed_migrations;
    embed_migrations!("migrations");
}

use async_std::path::PathBuf;
use clap::App;
use database::*;
use dotenv::dotenv;
use refinery::config::{Config, ConfigDbType};
use simple_error::SimpleError;
use util::*;

type SimpleResult<T> = Result<T, SimpleError>;

#[async_std::main]
async fn main() -> SimpleResult<()> {
    let matches = App::new(env!("CARGO_BIN_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .author(env!("CARGO_PKG_AUTHORS"))
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

        let data_directory = PathBuf::from(dirs::data_dir().unwrap()).join("oxyromon");
        create_directory(&data_directory).await?;

        let db_file = data_directory.join("oxyromon.db");
        let mut connection = establish_connection(db_file.as_os_str().to_str().unwrap()).await;

        let mut config =
            Config::new(ConfigDbType::Sqlite).set_db_path(db_file.as_os_str().to_str().unwrap());
        embedded::migrations::runner().run(&mut config).unwrap();

        match matches.subcommand_name() {
            Some("config") => {
                config::main(
                    &mut connection,
                    &matches.subcommand_matches("config").unwrap(),
                )
                .await?
            }
            Some("import-dats") => {
                import_dats::main(
                    &mut connection,
                    &matches.subcommand_matches("import-dats").unwrap(),
                )
                .await?
            }
            Some("import-roms") => {
                import_roms::main(
                    &mut connection,
                    &matches.subcommand_matches("import-roms").unwrap(),
                )
                .await?
            }
            Some("sort-roms") => {
                sort_roms::main(
                    &mut connection,
                    &matches.subcommand_matches("sort-roms").unwrap(),
                )
                .await?
            }
            Some("convert-roms") => {
                convert_roms::main(
                    &mut connection,
                    &matches.subcommand_matches("convert-roms").unwrap(),
                )
                .await?
            }
            Some("purge-roms") => {
                purge_roms::main(
                    &mut connection,
                    &matches.subcommand_matches("purge-roms").unwrap(),
                )
                .await?
            }
            _ => (),
        }

        close_connection(&mut connection).await;
    }

    Ok(())
}

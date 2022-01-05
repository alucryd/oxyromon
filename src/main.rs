extern crate async_std;
extern crate cfg_if;
extern crate clap;
extern crate crc32fast;
extern crate dialoguer;
extern crate digest;
extern crate dirs;
extern crate dotenv;
extern crate futures;
extern crate indicatif;
#[macro_use]
extern crate lazy_static;
extern crate once_cell;
extern crate quick_xml;
extern crate regex;
extern crate serde;
extern crate sqlx;
#[macro_use]
extern crate simple_error;
extern crate num_derive;
extern crate num_traits;
extern crate phf;
extern crate rayon;
extern crate surf;
extern crate tempfile;
extern crate vec_drain_where;

mod chdman;
mod check_roms;
mod checksum;
mod config;
mod convert_roms;
mod database;
mod download_dats;
mod import_dats;
mod import_roms;
mod maxcso;
mod model;
mod progress;
mod prompt;
mod purge_roms;
mod rebuild_roms;
#[cfg(feature = "server")]
mod server;
mod sevenzip;
mod sort_roms;
mod util;

use async_std::path::PathBuf;
use cfg_if::cfg_if;
use clap::App;
use database::*;
use dotenv::dotenv;
use progress::*;
use simple_error::SimpleError;
use util::*;

type SimpleResult<T> = Result<T, SimpleError>;

#[async_std::main]
#[allow(unused_mut)]
async fn main() -> SimpleResult<()> {
    let mut subcommands = vec![
        config::subcommand(),
        import_dats::subcommand(),
        download_dats::subcommand(),
        import_roms::subcommand(),
        sort_roms::subcommand(),
        convert_roms::subcommand(),
        rebuild_roms::subcommand(),
        check_roms::subcommand(),
        purge_roms::subcommand(),
    ];
    cfg_if! {
        if #[cfg(feature = "server")] {
            subcommands.push(server::subcommand());
        }
    }
    let matches = App::new(env!("CARGO_BIN_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .subcommands(subcommands)
        .get_matches();

    if matches.subcommand().is_some() {
        dotenv().ok();

        let progress_bar = get_progress_bar(0, get_none_progress_style());

        let data_directory = PathBuf::from(dirs::data_dir().unwrap()).join("oxyromon");
        create_directory(&progress_bar, &data_directory, true).await?;

        let db_file = data_directory.join("oxyromon.db");
        if !db_file.is_file().await {
            create_file(&progress_bar, &db_file, true).await?;
        }
        let pool = establish_connection(db_file.as_os_str().to_str().unwrap()).await;

        match matches.subcommand_name() {
            Some("config") => {
                config::main(
                    &mut pool.acquire().await.unwrap(),
                    &progress_bar,
                    matches.subcommand_matches("config").unwrap(),
                )
                .await?
            }
            Some("import-dats") => {
                import_dats::main(
                    &mut pool.acquire().await.unwrap(),
                    matches.subcommand_matches("import-dats").unwrap(),
                    &progress_bar,
                )
                .await?
            }
            Some("download-dats") => {
                download_dats::main(
                    &mut pool.acquire().await.unwrap(),
                    matches.subcommand_matches("download-dats").unwrap(),
                    &progress_bar,
                )
                .await?
            }
            Some("import-roms") => {
                import_roms::main(
                    &mut pool.acquire().await.unwrap(),
                    matches.subcommand_matches("import-roms").unwrap(),
                    &progress_bar,
                )
                .await?
            }
            Some("sort-roms") => {
                sort_roms::main(
                    &mut pool.acquire().await.unwrap(),
                    matches.subcommand_matches("sort-roms").unwrap(),
                    &progress_bar,
                )
                .await?
            }
            Some("convert-roms") => {
                convert_roms::main(
                    &mut pool.acquire().await.unwrap(),
                    matches.subcommand_matches("convert-roms").unwrap(),
                    &progress_bar,
                )
                .await?
            }
            Some("rebuild-roms") => {
                rebuild_roms::main(
                    &mut pool.acquire().await.unwrap(),
                    matches.subcommand_matches("rebuild-roms").unwrap(),
                    &progress_bar,
                )
                .await?
            }
            Some("check-roms") => {
                check_roms::main(
                    &mut pool.acquire().await.unwrap(),
                    matches.subcommand_matches("check-roms").unwrap(),
                    &progress_bar,
                )
                .await?
            }
            Some("purge-roms") => {
                purge_roms::main(
                    &mut pool.acquire().await.unwrap(),
                    matches.subcommand_matches("purge-roms").unwrap(),
                    &progress_bar,
                )
                .await?
            }
            Some("server") => {
                cfg_if! {
                    if #[cfg(feature = "server")] {
                        server::main(pool, matches.subcommand_matches("server").unwrap()).await?
                    }
                }
            }
            _ => (),
        }
        cfg_if! {
            if #[cfg(not(feature = "server"))] {
                close_connection(&pool).await;
            }
        }
    }

    Ok(())
}

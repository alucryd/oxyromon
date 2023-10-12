#[cfg(feature = "server")]
extern crate async_ctrlc;
#[cfg(feature = "server")]
extern crate async_graphql;
#[cfg(feature = "server")]
extern crate async_graphql_tide;
extern crate async_std;
#[cfg(feature = "server")]
extern crate async_trait;
extern crate cfg_if;
#[macro_use]
extern crate clap;
extern crate crc32fast;
extern crate dialoguer;
extern crate digest;
extern crate dirs;
extern crate dotenv;
extern crate env_logger;
extern crate futures;
#[cfg(feature = "server")]
extern crate http_types;
extern crate indicatif;
#[macro_use]
extern crate lazy_static;
extern crate log;
#[cfg(feature = "ird")]
extern crate md5;
extern crate num_derive;
extern crate num_traits;
extern crate once_cell;
extern crate phf;
extern crate quick_xml;
extern crate rayon;
extern crate regex;
extern crate rust_embed;
extern crate serde;
extern crate sha1;
#[macro_use]
extern crate simple_error;
extern crate sqlx;
#[cfg(feature = "ird")]
extern crate strsim;
extern crate strum;
extern crate surf;
extern crate tempfile;
#[cfg(feature = "server")]
extern crate tide;
extern crate vec_drain_where;
#[cfg(feature = "ird")]
extern crate walkdir;

#[cfg(feature = "benchmark")]
mod benchmark;
#[cfg(feature = "chd")]
mod chdman;
mod check_roms;
mod checksum;
#[cfg(feature = "cia")]
mod cia;
mod config;
mod convert_roms;
mod database;
#[cfg(feature = "rvz")]
mod dolphin;
mod download_dats;
mod generate_playlists;
mod import_dats;
#[cfg(feature = "ird")]
mod import_irds;
mod import_roms;
#[cfg(feature = "ird")]
mod isoinfo;
#[cfg(feature = "cso")]
mod maxcso;
mod model;
#[cfg(feature = "server")]
mod mutation;
#[cfg(feature = "nsz")]
mod nsz;
mod progress;
mod prompt;
mod purge_roms;
mod purge_systems;
#[cfg(feature = "server")]
mod query;
mod rebuild_roms;
#[cfg(feature = "server")]
mod server;
mod sevenzip;
mod sort_roms;
mod util;
#[cfg(feature = "server")]
mod validator;

use async_std::path::PathBuf;
use cfg_if::cfg_if;
use clap::Command;
use config::{get_rom_directory, get_tmp_directory};
use database::*;
use dotenv::dotenv;
use env_logger::{Builder, Target};
use progress::*;
use simple_error::SimpleError;
use std::env;
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
        purge_systems::subcommand(),
        generate_playlists::subcommand(),
    ];
    cfg_if! {
        if #[cfg(feature = "ird")] {
            subcommands.push(import_irds::subcommand());
        }
    }
    cfg_if! {
        if #[cfg(feature = "benchmark")] {
            subcommands.push(benchmark::subcommand());
        }
    }
    cfg_if! {
        if #[cfg(feature = "server")] {
            subcommands.push(server::subcommand());
        }
    }
    let matches = Command::new(env!("CARGO_BIN_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .subcommands(subcommands)
        .get_matches();

    if matches.subcommand().is_some() {
        dotenv().ok();

        let mut builder = Builder::from_env("OXYROMON_LOG");
        if matches.subcommand_name().unwrap() != "server" {
            // log to stdout for interactive commands because indicatif uses stderr
            builder.target(Target::Stdout);
        }
        builder.init();

        let progress_bar = get_progress_bar(0, get_none_progress_style());

        let data_directory = match env::var("OXYROMON_DATA_DIRECTORY") {
            Ok(data_directory) => PathBuf::from(data_directory),
            Err(_) => dirs::data_dir()
                .map(PathBuf::from)
                .unwrap()
                .join("oxyromon"),
        };
        create_directory(&progress_bar, &data_directory, true).await?;

        let db_file = data_directory.join("oxyromon.db");
        if !db_file.is_file().await {
            create_file(&progress_bar, &db_file, true).await?;
        }
        let pool = establish_connection(db_file.as_os_str().to_str().unwrap()).await;

        // make sure rom and tmp directories are initialized
        get_rom_directory(&mut pool.acquire().await.unwrap()).await;
        get_tmp_directory(&mut pool.acquire().await.unwrap()).await;

        match matches.subcommand_name() {
            Some("config") => {
                config::main(
                    &mut pool.acquire().await.unwrap(),
                    matches.subcommand_matches("config").unwrap(),
                    &progress_bar,
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
            Some("import-irds") => {
                cfg_if! {
                    if #[cfg(feature = "ird")] {
                        import_irds::main(
                            &mut pool.acquire().await.unwrap(),
                            matches.subcommand_matches("import-irds").unwrap(),
                            &progress_bar,
                        ).await?
                    }
                }
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
            Some("purge-systems") => {
                purge_systems::main(&mut pool.acquire().await.unwrap(), &progress_bar).await?
            }
            Some("generate-playlists") => {
                generate_playlists::main(
                    &mut pool.acquire().await.unwrap(),
                    matches.subcommand_matches("generate-playlists").unwrap(),
                    &progress_bar,
                )
                .await?
            }
            Some("benchmark") => {
                cfg_if! {
                    if #[cfg(feature = "benchmark")] {
                        benchmark::main(
                            &mut pool.acquire().await.unwrap(),
                            matches.subcommand_matches("benchmark").unwrap(),
                            &progress_bar,
                        ).await?
                    }
                }
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

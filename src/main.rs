#[cfg(feature = "server")]
extern crate async_graphql;
#[cfg(feature = "server")]
extern crate async_graphql_axum;
extern crate async_once_cell;
#[cfg(feature = "server")]
extern crate axum;
extern crate cfg_if;
extern crate chrono;
#[macro_use]
extern crate clap;
extern crate crc32fast;
extern crate dialoguer;
extern crate digest;
extern crate dirs;
extern crate dotenvy;
extern crate env_logger;
extern crate futures;
#[cfg(feature = "server")]
extern crate http_types;
extern crate indexmap;
extern crate indicatif;
extern crate infer;
#[macro_use]
extern crate lazy_static;
extern crate log;
extern crate md5;
extern crate num_derive;
extern crate num_traits;
extern crate phf;
extern crate quick_xml;
extern crate rayon;
extern crate regex;
extern crate reqwest;
extern crate rust_embed;
extern crate serde;
extern crate sha1;
#[macro_use]
extern crate simple_error;
extern crate sqlx;
extern crate strsim;
extern crate strum;
extern crate tempfile;
extern crate tokio;
extern crate vec_drain_where;
extern crate walkdir;
extern crate which;

mod bchunk;
mod benchmark;
mod chdman;
mod check_roms;
mod common;
mod config;
mod convert_roms;
mod crc32;
mod create_dats;
mod ctrtool;
mod database;
mod dolphin;
mod download_dats;
mod export_roms;
mod flips;
mod generate_playlists;
mod import_dats;
mod import_irds;
mod import_patches;
mod import_roms;
mod info;
mod maxcso;
mod mimetype;
mod model;
#[cfg(feature = "server")]
mod mutation;
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
mod wit;
mod xdelta3;

use cfg_if::cfg_if;
use clap::Command;
use config::{get_rom_directory, get_tmp_directory};
use database::*;
use dotenvy::dotenv;
use env_logger::{Builder, Target};
use progress::*;
use simple_error::SimpleError;
use std::env;
use std::path::PathBuf;
use util::*;

type SimpleResult<T> = Result<T, SimpleError>;

#[tokio::main]
#[allow(unused_mut)]
async fn main() -> SimpleResult<()> {
    let mut subcommands = vec![
        info::subcommand(),
        config::subcommand(),
        create_dats::subcommand(),
        import_dats::subcommand(),
        download_dats::subcommand(),
        import_irds::subcommand(),
        import_patches::subcommand(),
        import_roms::subcommand(),
        sort_roms::subcommand(),
        convert_roms::subcommand(),
        export_roms::subcommand(),
        rebuild_roms::subcommand(),
        check_roms::subcommand(),
        purge_roms::subcommand(),
        purge_systems::subcommand(),
        generate_playlists::subcommand(),
        benchmark::subcommand(),
    ];
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

        let mut builder = Builder::from_env("OXYROMON_LOG_LEVEL");
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
        if !db_file.is_file() {
            create_file(&progress_bar, &db_file, true).await?;
        }
        let pool = establish_connection(db_file.as_os_str().to_str().unwrap()).await;

        // make sure rom and tmp directories are initialized
        get_rom_directory(&mut pool.acquire().await.unwrap()).await;
        get_tmp_directory(&mut pool.acquire().await.unwrap()).await;

        match matches.subcommand_name() {
            Some("info") => info::main(&mut pool.acquire().await.unwrap(), &progress_bar).await?,
            Some("config") => {
                config::main(
                    &mut pool.acquire().await.unwrap(),
                    matches.subcommand_matches("config").unwrap(),
                    &progress_bar,
                )
                .await?
            }
            Some("create-dats") => {
                create_dats::main(
                    &mut pool.acquire().await.unwrap(),
                    matches.subcommand_matches("create-dats").unwrap(),
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
                import_irds::main(
                    &mut pool.acquire().await.unwrap(),
                    matches.subcommand_matches("import-irds").unwrap(),
                    &progress_bar,
                )
                .await?
            }
            Some("import-patches") => {
                import_patches::main(
                    &mut pool.acquire().await.unwrap(),
                    matches.subcommand_matches("import-patches").unwrap(),
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
            Some("export-roms") => {
                export_roms::main(
                    &mut pool.acquire().await.unwrap(),
                    matches.subcommand_matches("export-roms").unwrap(),
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
                purge_systems::main(
                    &mut pool.acquire().await.unwrap(),
                    matches.subcommand_matches("purge-systems").unwrap(),
                    &progress_bar,
                )
                .await?
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
                benchmark::main(
                    &mut pool.acquire().await.unwrap(),
                    matches.subcommand_matches("benchmark").unwrap(),
                    &progress_bar,
                )
                .await?
            }
            Some("server") => {
                cfg_if! {
                    if #[cfg(feature = "server")] {
                        server::main(pool.clone(), matches.subcommand_matches("server").unwrap()).await?
                    }
                }
            }
            _ => (),
        }
        optimize_database(pool).await;
    }

    Ok(())
}

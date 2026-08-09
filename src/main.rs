// the As*/To* romfile conversion traits intentionally take `self` by value to
// wrap it, mirroring the documented As*(parse)/To*(convert) naming convention
#![allow(clippy::wrong_self_convention)]

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
mod gdidrop;
mod generate_playlists;
mod import_dats;
mod import_irds;
mod import_patches;
mod import_roms;
mod info;
mod iso9660;
mod maxcso;
mod mimetype;
mod model;
#[cfg(feature = "server")]
mod mutation;
#[cfg(feature = "nod")]
mod nod;
mod nsz;
mod progress;
mod prompt;
mod purge_irds;
mod purge_roms;
mod purge_systems;
#[cfg(feature = "server")]
mod query;
mod rebuild_roms;
#[cfg(feature = "server")]
mod server;
#[cfg(feature = "sevenz")]
mod sevenz;
mod sevenzip;
mod sort_roms;
mod transcode;
mod util;
#[cfg(feature = "server")]
mod validator;
mod wit;
mod xdelta3;

use anyhow::Result;
use clap::Command;
use config::{get_rom_directory, get_tmp_directory};
use database::*;
use dotenvy::dotenv;
use env_logger::Builder;
use indicatif_log_bridge::LogWrapper;
use progress::*;
use std::env;
use std::path::PathBuf;
use util::*;

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg_attr(not(feature = "server"), allow(unused_mut))]
    let mut subcommands = vec![
        info::subcommand(),
        config::subcommand(),
        create_dats::subcommand(),
        import_dats::subcommand(),
        download_dats::subcommand(),
        import_irds::subcommand(),
        import_patches::subcommand(),
        import_roms::subcommand(),
        purge_irds::subcommand(),
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
    #[cfg(feature = "server")]
    subcommands.push(server::subcommand());
    let matches = Command::new(env!("CARGO_BIN_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .subcommands(subcommands)
        .get_matches();

    if let Some((subcommand, sub_matches)) = matches.subcommand() {
        dotenv().ok();

        let logger = Builder::from_env("OXYROMON_LOG_LEVEL").build();
        let level = logger.filter();
        let multi = get_multi_progress().clone();
        LogWrapper::new(multi, logger).try_init().unwrap();
        log::set_max_level(level);

        let progress_bar = get_progress_bar(0, get_none_progress_style());

        print_separator(&progress_bar);
        print_header(
            &progress_bar,
            &format!("oxyROMon {}", env!("CARGO_PKG_VERSION")),
        );
        print_separator(&progress_bar);

        let data_directory = match env::var("OXYROMON_DATA_DIRECTORY") {
            Ok(data_directory) => PathBuf::from(data_directory),
            Err(_) => dirs::data_dir().unwrap().join("oxyromon"),
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

        // the CLI pool only has a single connection, scope it so it is released
        // before optimize_database acquires one
        {
            let connection = &mut pool.acquire().await.unwrap();
            match subcommand {
                "info" => info::main(connection, &progress_bar).await?,
                "config" => config::main(connection, sub_matches, &progress_bar).await?,
                "create-dats" => create_dats::main(connection, sub_matches, &progress_bar).await?,
                "import-dats" => import_dats::main(connection, sub_matches, &progress_bar).await?,
                "download-dats" => {
                    download_dats::main(connection, sub_matches, &progress_bar).await?
                }
                "import-irds" => import_irds::main(connection, sub_matches, &progress_bar).await?,
                "purge-irds" => purge_irds::main(connection, sub_matches, &progress_bar).await?,
                "import-patches" => {
                    import_patches::main(connection, sub_matches, &progress_bar).await?
                }
                "import-roms" => import_roms::main(connection, sub_matches, &progress_bar).await?,
                "sort-roms" => sort_roms::main(connection, sub_matches, &progress_bar).await?,
                "convert-roms" => {
                    convert_roms::main(connection, sub_matches, &progress_bar).await?
                }
                "export-roms" => export_roms::main(connection, sub_matches, &progress_bar).await?,
                "rebuild-roms" => {
                    rebuild_roms::main(connection, sub_matches, &progress_bar).await?
                }
                "check-roms" => check_roms::main(connection, sub_matches, &progress_bar).await?,
                "purge-roms" => purge_roms::main(connection, sub_matches, &progress_bar).await?,
                "purge-systems" => {
                    purge_systems::main(connection, sub_matches, &progress_bar).await?
                }
                "generate-playlists" => {
                    generate_playlists::main(connection, sub_matches, &progress_bar).await?
                }
                "benchmark" => benchmark::main(connection, sub_matches, &progress_bar).await?,
                _ => (),
            }
        }
        #[cfg(feature = "server")]
        if subcommand == "server" {
            server::main(pool.clone(), sub_matches).await?;
        }
        optimize_database(pool).await;
    }

    Ok(())
}

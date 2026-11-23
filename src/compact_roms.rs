//! Compact the ORE stores of the systems selected: drop the chunks no game
//! uses any more, as `purge-roms` leaves them, pack small groups together,
//! and seal again with `ORE_STORE_PARITY` the segments sealed otherwise; or,
//! with `--recompress`, pack every group again, the smallest store for a
//! read and a write of all of it.

use super::config::*;
use super::ore::{OreProfile, compact_store, store_parity, store_settings_system_id, system_store};
use super::progress::*;
use super::prompt::*;
use anyhow::Result;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use sqlx::sqlite::SqliteConnection;
use std::collections::HashSet;
use std::str::FromStr;

pub fn subcommand() -> Command {
    Command::new("compact-roms")
        .about("Compact ORE stores, dropping what no game uses")
        .arg(
            Arg::new("ALL")
                .short('a')
                .long("all")
                .help("Compact all systems")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("RECOMPRESS")
                .short('r')
                .long("recompress")
                .help("Decode and pack every group again: the smallest store, for a read and a write of all of it")
                .required(false)
                .action(ArgAction::SetTrue),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> Result<()> {
    let systems =
        prompt_for_systems(connection, None, false, false, matches.get_flag("ALL")).await?;
    let recompress = matches.get_flag("RECOMPRESS");
    // A directory shared by systems holds one store, compacted once
    let mut done = HashSet::new();
    for system in systems {
        let store = system_store(connection, &system).await?;
        if !store.is_dir() || !done.insert(store.clone()) {
            continue;
        }
        print_header(progress_bar, &format!("Processing \"{}\"", system.name));
        let settings_id = store_settings_system_id(connection, &system).await?;
        let profile = OreProfile::from_str(
            &get_string(connection, "ORE_PROFILE", Some(settings_id))
                .await
                .unwrap(),
        )
        .unwrap();
        let parity = store_parity(connection, settings_id).await;
        compact_store(progress_bar, &store, &profile, parity, recompress).await?;
        print_separator(progress_bar);
    }
    if done.is_empty() {
        print_info(progress_bar, "No ORE store found");
    }
    Ok(())
}

#[cfg(test)]
mod tests;

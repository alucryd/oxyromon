use super::archive;
use super::chd;
use super::cia;
use super::database::*;
use super::nsz;
use super::progress::*;
use super::rvz;
use super::wbfs;
use super::wux;
use super::xdelta;
use super::xps;
use super::xso;
use anyhow::Result;
use clap::Command;
use indicatif::ProgressBar;
use sqlx::sqlite::SqliteConnection;

pub fn subcommand() -> Command {
    Command::new("info").about("Print system information")
}

/// The libraries each format comes from, by name, with their versions; shared
/// by `info`, GraphQL and MCP.
pub async fn dependencies() -> Vec<(&'static str, Result<String>)> {
    let mut deps = vec![
        ("sevenz-rust2", archive::get_version().await),
        ("chd-rs", chd::get_version().await),
        ("cia-rs", cia::get_version().await),
        ("xps-rs", xps::get_version().await),
        ("xso-rs", xso::get_version().await),
        ("nsz-rs", nsz::get_version().await),
        ("xdelta-rs", xdelta::get_version().await),
        ("wux-rs", wux::get_version().await),
        ("rvz-rs", rvz::get_version().await),
        ("wbfs-rs", wbfs::get_version().await),
    ];
    deps.sort_by_key(|(name, _)| *name);
    deps
}

pub async fn main(connection: &mut SqliteConnection, progress_bar: &ProgressBar) -> Result<()> {
    start_action(progress_bar, None);

    // Dependencies
    print_header(progress_bar, "Dependencies");

    for (name, result) in &dependencies().await {
        match result {
            Ok(version) => print_success(progress_bar, &format!("{}: {}", name, version)),
            Err(_) => print_skip(progress_bar, &format!("{}: not found", name)),
        }
    }

    print_separator(progress_bar);

    // Statistics
    print_header(progress_bar, "Statistics");

    let system_count = count_systems(connection).await;
    let game_count = count_games(connection).await;
    let rom_count = count_roms(connection).await;

    print_info(progress_bar, &format!("Systems: {}", system_count));
    print_info(progress_bar, &format!("Games:   {}", game_count));
    print_info(progress_bar, &format!("ROMs:    {}", rom_count));

    print_separator(progress_bar);

    print_line(
        progress_bar,
        "  If you find oxyromon useful, please consider buying me a coffee:",
    );
    print_line(progress_bar, "  https://ko-fi.com/alucryd");

    progress_bar.finish_and_clear();

    Ok(())
}

#[cfg(test)]
mod test_info;

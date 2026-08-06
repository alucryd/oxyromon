use super::bchunk;
use super::chdman;
use super::ctrtool;
use super::database::*;
use super::dolphin;
use super::flips;
use super::maxcso;
use super::nsz;
use super::progress::*;
use super::sevenzip;
use super::util::*;
use super::wit;
use super::xdelta3;
use anyhow::Result;
use clap::Command;
use indicatif::ProgressBar;
use sqlx::sqlite::SqliteConnection;
use std::time::Duration;

pub fn subcommand() -> Command {
    Command::new("info").about("Print system information")
}

pub async fn main(connection: &mut SqliteConnection, progress_bar: &ProgressBar) -> Result<()> {
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    // Dependencies
    print_header(progress_bar, "Dependencies");

    let deps: Vec<(&str, Result<String, _>)> = vec![
        ("7-zip", sevenzip::get_version().await),
        ("bchunk", bchunk::get_version().await),
        ("chdman", chdman::get_version().await),
        ("ctrtool", ctrtool::get_version().await),
        ("dolphin-tool", dolphin::get_version().await),
        ("flips", flips::get_version().await),
        ("maxcso", maxcso::get_version().await),
        ("nsz", nsz::get_version().await),
        ("wit", wit::get_version().await),
        ("xdelta3", xdelta3::get_version().await),
    ];

    for (name, result) in &deps {
        print_dependency(progress_bar, name, result);
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

    progress_bar.println("  If you find oxyromon useful, please consider buying me a coffee:");
    progress_bar.println("  https://ko-fi.com/alucryd");

    progress_bar.finish_and_clear();

    Ok(())
}

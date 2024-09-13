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
use super::wit;
use super::xdelta3;
use super::SimpleResult;
use clap::Command;
use indicatif::ProgressBar;
use sqlx::sqlite::SqliteConnection;
use std::time::Duration;

pub fn subcommand() -> Command {
    Command::new("info").about("Print system information")
}

pub async fn main(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    progress_bar.set_style(get_none_progress_style());
    progress_bar.enable_steady_tick(Duration::from_millis(100));

    progress_bar.println(format!("Version: {}", env!("CARGO_PKG_VERSION")));
    progress_bar.println("");

    let sevenzip_version = match sevenzip::get_version().await {
        Ok(version) => format!("found ({})", version),
        Err(_) => String::from("not found"),
    };
    let bchunk_version = match bchunk::get_version().await {
        Ok(version) => format!("found ({})", version),
        Err(_) => String::from("not found"),
    };
    let chdman_version = match chdman::get_version().await {
        Ok(version) => format!("found ({})", version),
        Err(_) => String::from("not found"),
    };
    let ctrtool_version = match ctrtool::get_version().await {
        Ok(version) => format!("found ({})", version),
        Err(_) => String::from("not found"),
    };
    let dolphin_version = match dolphin::get_version().await {
        Ok(version) => format!("found ({})", version),
        Err(_) => String::from("not found"),
    };
    let flips_version = match flips::get_version().await {
        Ok(version) => format!("found ({})", version),
        Err(_) => String::from("not found"),
    };
    let maxcso_version = match maxcso::get_version().await {
        Ok(version) => format!("found ({})", version),
        Err(_) => String::from("not found"),
    };
    let nsz_version = match nsz::get_version().await {
        Ok(version) => format!("found ({})", version),
        Err(_) => String::from("not found"),
    };
    let wit_version = match wit::get_version().await {
        Ok(version) => format!("found ({})", version),
        Err(_) => String::from("not found"),
    };
    let xdelta3_version = match xdelta3::get_version().await {
        Ok(version) => format!("found ({})", version),
        Err(_) => String::from("not found"),
    };

    progress_bar.println("Dependencies:");
    progress_bar.println(format!("  7-zip: {}", sevenzip_version));
    progress_bar.println(format!("  bchunk: {}", bchunk_version));
    progress_bar.println(format!("  chdman: {}", chdman_version));
    progress_bar.println(format!("  ctrtool: {}", ctrtool_version));
    progress_bar.println(format!("  dolphin: {}", dolphin_version));
    progress_bar.println(format!("  flips: {}", flips_version));
    progress_bar.println(format!("  maxcso: {}", maxcso_version));
    progress_bar.println(format!("  nsz: {}", nsz_version));
    progress_bar.println(format!("  wit: {}", wit_version));
    progress_bar.println(format!("  xdelta3: {}", xdelta3_version));
    progress_bar.println("");

    let system_count = count_systems(connection).await;
    let game_count = count_games(connection).await;
    let rom_count = count_roms(connection).await;

    progress_bar.println(format!("Systems: {}", system_count));
    progress_bar.println(format!("Games: {}", game_count));
    progress_bar.println(format!("Roms: {}", rom_count));

    Ok(())
}

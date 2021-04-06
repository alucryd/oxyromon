use super::config::*;
use super::database::*;
use super::import_roms::import_rom;
use super::model::*;
use super::progress::*;
use super::prompt::*;
use super::util::*;
use super::SimpleResult;
use async_std::io;
use async_std::path::{Path, PathBuf};
use clap::{App, Arg, ArgMatches, SubCommand};
use indicatif::ProgressBar;
use phf::phf_map;
use quick_xml::de;
use rayon::prelude::*;
use regex::Regex;
use shiratsu_naming::naming::nointro::{NoIntroName, NoIntroToken};
use shiratsu_naming::naming::TokenizedName;
use shiratsu_naming::region::Region;
use sqlx::SqliteConnection;
use surf;

static REDUMP_URLS: phf::Map<&'static str, &'static str> = phf_map! {
    "Nintendo - GameCube" => "http://redump.org/datfile/gc/",
};

pub fn subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name("update-dats")
        .about("Updates No-Intro and Redump DAT files")
        .arg(
            Arg::with_name("ALL")
                .short("a")
                .long("all")
                .help("Sorts all systems")
                .required(false),
        )
        .arg(
            Arg::with_name("YES")
                .short("y")
                .long("yes")
                .help("Automatically says yes to prompts")
                .required(false),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches<'_>,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let systems = prompt_for_systems(connection, matches.is_present("ALL"), &progress_bar).await;

    for system in systems {
        update_dat(connection, &system, progress_bar).await?;
    }

    Ok(())
}

pub async fn update_dat(
    connection: &mut SqliteConnection,
    system: &System,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    if let Some(url) = &system.url {
        let zip_url = match url.as_str() {
            "http://redump.org/" => REDUMP_URLS.get(system.name.as_str()).cloned().unwrap(),
            _ => panic!(""),
        };
        let response = surf::get(zip_url)
            .await
            .expect("Failed to download updated DAT");
        let tmp_directory = get_tmp_directory(connection).await;
        let zip = create_file(&tmp_directory.join("update.zip")).await?;
        try_with!(io::copy(response, zip).await, "Failed to write ZIP");
    } else {
        progress_bar.println(&format!("{} has no url", system.name));
    };

    Ok(())
}

#[cfg(test)]
mod test {
    use super::super::database::*;
    use super::*;
    use async_std::fs;
    use async_std::path::Path;
    use async_std::sync::Mutex;
    use tempfile::{NamedTempFile, TempDir};
}

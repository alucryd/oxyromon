use super::import_dats::import_dat;
use super::model::*;
use super::prompt::*;
use super::util::*;
use super::SimpleResult;
use clap::{App, Arg, ArgMatches, SubCommand};
use indicatif::ProgressBar;
use phf::phf_map;
use sqlx::SqliteConnection;
use std::io::Cursor;
use surf;
use zip::read::ZipArchive;

static REDUMP_CODES: phf::Map<&'static str, &'static str> = phf_map! {
    "Microsoft - Xbox" => "xbox",
    "NEC - PC Engine CD & TurboGrafx CD" => "pce",
    "Nintendo - GameCube" => "gc",
    "Sega - Dreamcast" => "dc",
    "Sega - Mega CD & Sega CD" => "mcd",
    "Sega - Saturn" => "ss",
    "SNK - Neo Geo CD" => "ngcd",
    "Sony - PlayStation" => "psx",
    "Sony - PlayStation 2" => "ps2",
    "Sony - PlayStation Portable" => "psp",
};

pub fn subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name("update-dats")
        .about("Updates No-Intro and Redump DAT files")
        .arg(
            Arg::with_name("ALL")
                .short("a")
                .long("all")
                .help("Updates all systems")
                .required(false),
        )
        .arg(
            Arg::with_name("FORCE")
                .short("f")
                .long("force")
                .help("Forces import of outdated DAT files")
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
        if let Some(url) = &system.url {
            match url.as_str() {
                "http://redump.org/" => {
                    update_redump_dat(
                        connection,
                        progress_bar,
                        &system,
                        matches.is_present("FORCE"),
                    )
                    .await?
                }
                url => progress_bar.println(format!("Updating from {} is unsupported", url)),
            }
        } else {
            progress_bar.println("System has no URL");
        }
    }

    Ok(())
}

async fn update_redump_dat(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    force: bool,
) -> SimpleResult<()> {
    match REDUMP_CODES.get(system.name.as_str()) {
        Some(code) => {
            let zip_url = format!("{}datfile/{}/", &system.url.as_ref().unwrap(), code);
            let response = surf::get(zip_url)
                .recv_bytes()
                .await
                .expect("Failed to download ZIP");
            let tmp_directory = create_tmp_directory(connection).await?;
            let mut zip_archive =
                try_with!(ZipArchive::new(Cursor::new(response)), "Failed to read ZIP");
            match zip_archive.len() {
                0 => progress_bar.println("Update ZIP is empty"),
                1 => {
                    try_with!(zip_archive.extract(&tmp_directory), "Failed to extract ZIP");
                    import_dat(
                        connection,
                        progress_bar,
                        &tmp_directory
                            .path()
                            .join(zip_archive.file_names().next().unwrap()),
                        false,
                        true,
                        force,
                    )
                    .await?;
                }
                _ => progress_bar.println("Update ZIP contains too many files"),
            }
        }
        None => progress_bar.println("System is unsupported (yet)"),
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

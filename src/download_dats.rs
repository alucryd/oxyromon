use super::SimpleResult;
use super::database::*;
use super::import_dats::{import_dat, parse_dat};
use super::model::*;
use super::prompt::*;
use super::util::*;
use cfg_if::cfg_if;
use clap::{Arg, ArgAction, ArgMatches, Command};
use indicatif::ProgressBar;
use phf::phf_map;
use quick_xml::de;
use rayon::prelude::*;
use sqlx::sqlite::SqliteConnection;
use std::collections::HashSet;
use std::io::Cursor;
use std::path::PathBuf;
use tokio::time::{Duration, sleep};
use zip::read::ZipArchive;

const NOINTRO_BASE_URL: &str = "https://datomatic.no-intro.org";
const NOINTRO_PROFILE_URL: &str = "/profile.xml";
const REDUMP_BASE_URL: &str = "http://redump.org";

pub const NOINTRO_SYSTEM_URL: &str = "www.no-intro.org";
pub const REDUMP_SYSTEM_URL: &str = "http://redump.org/";

cfg_if! {
    if #[cfg(test)] {
        static REDUMP_SYSTEMS_CODES: phf::Map<&str, &str> = phf_map! {
            "Test System" => "ts"
        };
    } else {
        static REDUMP_SYSTEMS_CODES: phf::Map<&str, &str> = phf_map! {
            "Acorn - Archimedes" => "arch",
            "Apple - Macintosh" => "mac",
            "Arcade - Konami - e-Amusement" => "kea",
            "Arcade - Konami - FireBeat" => "kfb",
            "Arcade - Konami - System GV" => "ksgv",
            "Arcade - Namco - Sega - Nintendo - Triforce" => "trf",
            "Arcade - Sega - Chihiro" => "chihiro",
            "Arcade - Sega - Lindbergh" => "lindbergh",
            "Arcade - Sega - Naomi" => "naomi",
            "Arcade - Sega - Naomi 2" => "naomi2",
            "Arcade - Sega - RingEdge" => "sre",
            "Arcade - Sega - RingEdge 2" => "sre2",
            "Atari - Jaguar CD Interactive Multimedia System" => "ajcd",
            "Bandai - Pippin" => "pippin",
            "Bandai - Playdia Quick Interactive System" => "qis",
            "Commodore - Amiga CD" => "acd",
            "Commodore - Amiga CD32" => "cd32",
            "Commodore - Amiga CDTV" => "cdtv",
            "Fujitsu - FM-Towns" => "fmt",
            "funworld - Photo Play" => "fpp",
            "IBM - PC compatible" => "pc",
            "Incredible Technologies - Eagle" => "ite",
            "Mattel - Fisher-Price iXL" => "ixl",
            "Mattel - HyperScan" => "hs",
            "Memorex - Visual Information System" => "vis",
            "Microsoft - Xbox" => "xbox",
            "Microsoft - Xbox 360" => "xbox360",
            "NEC - PC Engine CD & TurboGrafx CD" => "pce",
            "NEC - PC-88 series" => "pc-88",
            "NEC - PC-98 series" => "pc-98",
            "NEC - PC-FX & PC-FXGA" => "pc-fx",
            "Nintendo - GameCube" => "gc",
            "Nintendo - Wii" => "wii",
            "Palm" => "palm",
            "Panasonic - 3DO Interactive Multiplayer" => "3do",
            "Philips - CD-i" => "cdi",
            "Photo CD" => "photo-cd",
            "PlayStation GameShark Updates" => "psxgs",
            "Sega - Dreamcast" => "dc",
            "Sega - Mega CD & Sega CD" => "mcd",
            "Sega - Prologue 21" => "sp21",
            "Sega - Saturn" => "ss",
            "SNK - Neo Geo CD" => "ngcd",
            "Sony - PlayStation" => "psx",
            "Sony - PlayStation 2" => "ps2",
            "Sony - PlayStation 3" => "ps3",
            "Sony - PlayStation Portable" => "psp",
            "TAB-Austria - Quizard" => "quizard",
            "Tomy - Kiss-Site" => "ksite",
            "VM Labs - NUON" => "nuon",
            "VTech - V.Flash & V.Smile Pro" => "vflash",
            "ZAPiT Games - Game Wave Family Entertainment System" => "gamewave",
        };
    }
}

pub fn subcommand() -> Command {
    Command::new("download-dats")
        .about("Download No-Intro and Redump DAT files and import them into oxyromon")
        .arg(
            Arg::new("NOINTRO")
                .short('n')
                .long("nointro")
                .help("Download No-Intro DAT files")
                .required(false)
                .action(ArgAction::SetTrue)
                .conflicts_with("REDUMP")
                .required_unless_present("REDUMP"),
        )
        .arg(
            Arg::new("REDUMP")
                .short('r')
                .long("redump")
                .help("Download Redump DAT files")
                .required(false)
                .action(ArgAction::SetTrue)
                .conflicts_with("NOINTRO")
                .required_unless_present("NOINTRO"),
        )
        .arg(
            Arg::new("UPDATE")
                .short('u')
                .long("update")
                .help("Check for system updates")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("ALL")
                .short('a')
                .long("all")
                .help("Import all systems")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("FORCE")
                .short('f')
                .long("force")
                .help("Force import of outdated DAT files")
                .required(false)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("SAVE_DIRECTORY")
                .short('s')
                .long("save")
                .help("Save downloaded DAT files to the specified directory")
                .required(false)
                .value_name("DIRECTORY"),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    let save_directory = matches.get_one::<String>("SAVE_DIRECTORY");

    if matches.get_flag("NOINTRO") {
        if matches.get_flag("UPDATE") {
            update_nointro_dats(
                connection,
                progress_bar,
                NOINTRO_BASE_URL,
                matches.get_flag("ALL"),
            )
            .await?
        } else {
            progress_bar.println("Not supported");
        }
    } else if matches.get_flag("REDUMP") {
        if matches.get_flag("UPDATE") {
            update_redump_dats(
                connection,
                progress_bar,
                REDUMP_BASE_URL,
                matches.get_flag("ALL"),
                matches.get_flag("FORCE"),
                save_directory.map(|s| s.as_str()),
            )
            .await?
        } else {
            download_redump_dats(
                connection,
                progress_bar,
                REDUMP_BASE_URL,
                matches.get_flag("ALL"),
                save_directory.map(|s| s.as_str()),
            )
            .await?
        }
    }
    Ok(())
}

async fn update_nointro_dats(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    base_url: &str,
    all: bool,
) -> SimpleResult<()> {
    if let Ok(response) = reqwest::get(format!("{}{}", base_url, NOINTRO_PROFILE_URL))
        .await
        .expect("Failed to download No-Intro profiles")
        .text()
        .await
    {
        let profile: ProfileXml =
            try_with!(de::from_str(&response), "Failed to parse No-Intro profiles");
        let systems =
            prompt_for_systems(connection, Some(NOINTRO_SYSTEM_URL), false, false, all).await?;
        for system in systems {
            progress_bar.println(format!("Processing \"{}\"", &system.name));
            let system_xml = profile
                .systems
                .par_iter()
                .find_first(|system_xml| system_xml.name == system.name);
            match system_xml {
                Some(system_xml) => {
                    is_update(progress_bar, &system.version, &system_xml.version);
                }
                None => progress_bar.println("System is no longer available"),
            }
            progress_bar.println("");
        }
    }
    Ok(())
}

async fn download_redump_dats(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    base_url: &str,
    all: bool,
    save_directory: Option<&str>,
) -> SimpleResult<()> {
    let system_names: HashSet<String> = find_systems_by_url(connection, REDUMP_SYSTEM_URL)
        .await
        .into_par_iter()
        .map(|system| system.name)
        .collect();
    let mut items: Vec<&str> = REDUMP_SYSTEMS_CODES
        .keys()
        .copied()
        .filter(|&system_name| !system_names.contains(system_name))
        .collect();
    items.sort_unstable();
    let indices: Vec<usize> = if all {
        (0..items.len()).collect()
    } else {
        multiselect(&items, "Please select systems", None, None)?
    };
    for i in indices {
        download_redump_dat(
            connection,
            progress_bar,
            base_url,
            items.get(i).unwrap(),
            false,
            save_directory,
        )
        .await?;
    }
    Ok(())
}

async fn update_redump_dats(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    base_url: &str,
    all: bool,
    force: bool,
    save_directory: Option<&str>,
) -> SimpleResult<()> {
    let systems =
        prompt_for_systems(connection, Some(REDUMP_SYSTEM_URL), false, false, all).await?;
    for system in systems {
        download_redump_dat(
            connection,
            progress_bar,
            base_url,
            &system.name,
            force,
            save_directory,
        )
        .await?;
    }
    Ok(())
}

async fn download_redump_dat(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    base_url: &str,
    system_name: &str,
    force: bool,
    save_directory: Option<&str>,
) -> SimpleResult<()> {
    progress_bar.println(format!("Processing \"{}\"", system_name));
    let code = *REDUMP_SYSTEMS_CODES.get(system_name).unwrap();
    let zip_url = format!("{}/datfile/{}/", base_url, code);
    match reqwest::get(zip_url)
        .await
        .expect("Failed to download ZIP")
        .bytes()
        .await
    {
        Ok(response) => {
            let tmp_directory = create_tmp_directory(connection).await?;
            let mut zip_archive = try_with!(
                ZipArchive::new(Cursor::new(response)),
                "Failed to read Redump ZIP"
            );
            match zip_archive.len() {
                0 => progress_bar.println("ZIP is empty"),
                1 => {
                    try_with!(zip_archive.extract(&tmp_directory), "Failed to extract ZIP");
                    let dat_file_name = zip_archive.file_names().next().unwrap();
                    let dat_file_path = tmp_directory.path().join(dat_file_name);

                    // Save DAT file to specified directory if requested
                    if let Some(save_directory) = save_directory {
                        let save_directory = PathBuf::from(save_directory);
                        create_directory(progress_bar, &save_directory, false).await?;
                        copy_file(
                            progress_bar,
                            &dat_file_path,
                            &save_directory.join(dat_file_name),
                            false,
                        )
                        .await?;
                    }

                    let (datfile_xml, detector_xml) =
                        parse_dat(progress_bar, &dat_file_path, true).await?;
                    import_dat(
                        connection,
                        progress_bar,
                        &datfile_xml,
                        &detector_xml,
                        None,
                        None,
                        force,
                    )
                    .await?;
                }
                _ => progress_bar.println("ZIP contains too many files"),
            }
        }
        _ => progress_bar.println("Failed to download ZIP"),
    }
    // rate limit
    sleep(Duration::from_secs(1)).await;
    progress_bar.println("");
    Ok(())
}

#[cfg(test)]
mod test_nointro;
#[cfg(test)]
mod test_redump;

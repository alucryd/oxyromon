use super::database::*;
use super::import_dats::{import_dat, parse_dat};
use super::model::*;
use super::progress::*;
use super::prompt::*;
use super::util::*;
use anyhow::{Context, Result, bail};
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
const REDUMP_BASE_URL: &str = "https://redump.info";

pub const NOINTRO_SYSTEM_URL: &str = "www.no-intro.org";
pub const REDUMP_SYSTEM_URL: &str = "https://redump.info/";

cfg_if! {
    if #[cfg(test)] {
        static REDUMP_SYSTEMS_CODES: phf::Map<&str, &str> = phf_map! {
            "Test System" => "TS"
        };
    } else {
        static REDUMP_SYSTEMS_CODES: phf::Map<&str, &str> = phf_map! {
            "3DO Interactive Multiplayer" => "3DO",
            "Acorn - Archimedes & Risc PC" => "ARCH",
            "Apple - Macintosh" => "MAC",
            "Apple - Pippin" => "PIPPIN",
            "Arcade - American Laser Games - 3DO-based Arcade" => "3DOARCADE",
            "Arcade - Capcom - Play System III" => "CPS3",
            "Arcade - FuRyu & Omron - Purikura" => "FPURI",
            "Arcade - Funworld - Photo Play" => "FPP",
            "Arcade - Incredible Technologies - Eagle" => "ITE",
            "Arcade - Konami - FireBeat" => "KFB",
            "Arcade - Konami - M2" => "KM2",
            "Arcade - Konami - Python 2" => "KP2",
            "Arcade - Konami - System 573" => "KS573",
            "Arcade - Konami - System GV" => "KSGV",
            "Arcade - Konami - e-Amusement" => "KEA",
            "Arcade - Merit Industries - Megatouch ION" => "MION",
            "Arcade - Namco - Purikura" => "NPURI",
            "Arcade - Namco - Sega - Nintendo - Triforce" => "TRF",
            "Arcade - Namco - System 12" => "NS12",
            "Arcade - Namco - System 22" => "NS22",
            "Arcade - Namco - System 246" => "NS246",
            "Arcade - Namco - System 256" => "NS256",
            "Arcade - New Jatre - CD-i based Arcade" => "NEWJATRE",
            "Arcade - PC-based Arcade" => "PCARCADE",
            "Arcade - Sega - ALLS" => "ALLS",
            "Arcade - Sega - Chihiro" => "CHIHIRO",
            "Arcade - Sega - Chihiro Satellite Terminal PC" => "CHIHIROPC",
            "Arcade - Sega - Lindbergh" => "LINDBERGH",
            "Arcade - Sega - Lindbergh Satellite Terminal PC" => "LINDPC",
            "Arcade - Sega - Naomi" => "NAOMI",
            "Arcade - Sega - Naomi 2" => "NAOMI2",
            "Arcade - Sega - Naomi Satellite Terminal PC" => "NAOMIPC",
            "Arcade - Sega - Nu" => "NU",
            "Arcade - Sega - Nu 1.1" => "NU11",
            "Arcade - Sega - Nu 2" => "NU2",
            "Arcade - Sega - Nu SX" => "NUSX",
            "Arcade - Sega - RingEdge" => "SRE",
            "Arcade - Sega - RingEdge 2" => "SRE2",
            "Arcade - Sega - RingWide" => "SRW",
            "Arcade - Seibu - CATS E-Touch" => "CATS",
            "Arcade - TAB-Austria - Quizard" => "QUIZARD",
            "Atari - Jaguar CD Interactive Multimedia System" => "AJCD",
            "Atari - ST series" => "ATARIST",
            "Audio CD" => "AUDIO-CD",
            "BD-Video" => "BD-VIDEO",
            "Bandai - Playdia Quick Interactive System" => "QIS",
            "Commodore - Amiga CD" => "ACD",
            "Commodore - Amiga CD32" => "CD32",
            "Commodore - Amiga CDTV" => "CDTV",
            "DVD-Video" => "DVD-VIDEO",
            "Datel - PlayStation Cheat Device Updates" => "PSXGS",
            "Enhanced CD" => "ENHANCED-CD",
            "Fujitsu - FM Towns series" => "FMT",
            "HD DVD-Video" => "HDDVD-VIDEO",
            "Handheld - Apple - Newton" => "NEWTON",
            "Handheld - Cybiko" => "CYBIKO",
            "Handheld - Mattel - Fisher-Price iXL" => "IXL",
            "Handheld - Microsoft - Pocket PC" => "PPC",
            "Handheld - Palm OS" => "PALM",
            "Handheld - Psion" => "PSION",
            "Handheld - Sharp - Zaurus" => "ZAURUS",
            "Handheld - Symbian" => "SYMBIAN",
            "Handheld - Texas Instruments - TI series" => "TI",
            "Hasbro - VideoNow" => "HVN",
            "Hasbro - VideoNow Color" => "HVNC",
            "Hasbro - VideoNow Jr." => "HVNJR",
            "Hasbro - VideoNow XP" => "HVNXP",
            "Hasbro - iON Educational Gaming System" => "ION",
            "IBM - PC compatible" => "PC",
            "MP3 Audio Disc" => "MP3",
            "Mattel - HyperScan" => "HS",
            "Memorex - Visual Information System" => "VIS",
            "Microsoft - Xbox" => "XBOX",
            "Microsoft - Xbox 360" => "XBOX360",
            "Microsoft - Xbox One" => "XBOXONE",
            "Microsoft - Xbox Series X" => "XBOXSX",
            "NEC - PC Engine CD & TurboGrafx CD" => "PCE",
            "NEC - PC-88 series" => "PC-88",
            "NEC - PC-98 series" => "PC-98",
            "NEC - PC-FX & PC-FXGA" => "PC-FX",
            "Navisoft - Naviken" => "NAVI",
            "Nintendo - GameCube" => "GC",
            "Nintendo - Wii" => "WII",
            "Nintendo - Wii U" => "WIIU",
            "Panasonic - M2" => "M2",
            "Philips - CD-i" => "CDI",
            "Photo CD" => "PHOTO-CD",
            "Playmaji - Polymega" => "POLYMEGA",
            "SGI - Indigo series" => "INDIGO",
            "SNK - Neo Geo CD" => "NGCD",
            "Sega - Dreamcast" => "DC",
            "Sega - Mega CD & Sega CD" => "MCD",
            "Sega - Prologue 21 Multimedia Karaoke System" => "SP21",
            "Sega - Saturn" => "SS",
            "Sharp - X68000" => "X68K",
            "Sony - Electronic Book" => "SEB",
            "Sony - PlayStation" => "PSX",
            "Sony - PlayStation 2" => "PS2",
            "Sony - PlayStation 3" => "PS3",
            "Sony - PlayStation 4" => "PS4",
            "Sony - PlayStation 5" => "PS5",
            "Sony - PlayStation Portable" => "PSP",
            "Sun Microsystems - Ultra" => "ULTRA",
            "Tomy - Kiss-Site" => "KSITE",
            "VM Labs - Nuon" => "NUON",
            "VTech - V.Flash & V.Smile Pro" => "VFLASH",
            "Video CD" => "VCD",
            "ZAPiT Games - Game Wave Family Entertainment System" => "GAMEWAVE",
        };
    }
}

/// The Redump systems oxyromon knows how to fetch, sorted by name.
///
/// `update` picks which half of the catalogue is wanted: the systems already in
/// the database, whose DATs can be refreshed, or the ones that are not there
/// yet. Either way the answer is drawn from the catalogue, so a system imported
/// from a hand-downloaded Redump DAT under some other name is never offered for
/// an update oxyromon has no code for.
///
/// This is what `download-dats -r` offers interactively, with and without `-u`;
/// the server exposes the same lists so the web UI can offer them as a picker.
#[cfg(any(feature = "server", feature = "mcp"))]
pub async fn find_redump_systems(connection: &mut SqliteConnection, update: bool) -> Vec<String> {
    let existing: HashSet<String> = find_systems_by_url(connection, REDUMP_SYSTEM_URL)
        .await
        .into_par_iter()
        .map(|system| system.name)
        .collect();
    let mut names: Vec<String> = REDUMP_SYSTEMS_CODES
        .keys()
        .filter(|name| existing.contains(**name) == update)
        .map(|name| name.to_string())
        .collect();
    names.sort_unstable();
    names
}

/// Fetch the named Redump systems' DATs and import them.
///
/// Unlike the CLI, which can only offer names it already holds, callers here
/// may pass anything: an unknown name lands in `failed`.
#[cfg(any(feature = "server", feature = "mcp"))]
pub async fn download_redump_systems(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system_names: &[String],
    force: bool,
) -> DatOutcomes {
    collect_redump_dats(
        connection,
        progress_bar,
        REDUMP_BASE_URL,
        system_names,
        force,
        None,
    )
    .await
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
) -> Result<()> {
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
            print_warning(progress_bar, "Not supported");
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
) -> Result<()> {
    let profile_url = format!("{}{}", base_url, NOINTRO_PROFILE_URL);
    let response = reqwest::get(&profile_url)
        .await
        .and_then(|response| response.error_for_status())
        .with_context(|| format!("Failed to download \"{profile_url}\""))?
        .text()
        .await
        .with_context(|| format!("Failed to read \"{profile_url}\""))?;
    let profile: ProfileXml =
        de::from_str(&response).context("Failed to parse No-Intro profiles")?;
    let systems =
        prompt_for_systems(connection, Some(NOINTRO_SYSTEM_URL), false, false, all).await?;
    for system in systems {
        print_header(progress_bar, &format!("Processing \"{}\"", system.name));
        let system_xml = profile
            .systems
            .par_iter()
            .find_first(|system_xml| system_xml.name == system.name);
        match system_xml {
            Some(system_xml) => {
                is_update(progress_bar, &system.version, &system_xml.version);
            }
            None => print_warning(progress_bar, "System is no longer available"),
        }
        print_separator(progress_bar);
    }
    Ok(())
}

async fn download_redump_dats(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    base_url: &str,
    all: bool,
    save_directory: Option<&str>,
) -> Result<()> {
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
    let system_names: Vec<&str> = indices.into_iter().map(|i| items[i]).collect();
    download_redump_batch(
        connection,
        progress_bar,
        base_url,
        &system_names,
        false,
        save_directory,
    )
    .await
}

async fn update_redump_dats(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    base_url: &str,
    all: bool,
    force: bool,
    save_directory: Option<&str>,
) -> Result<()> {
    let systems =
        prompt_for_systems(connection, Some(REDUMP_SYSTEM_URL), false, false, all).await?;
    let system_names: Vec<&str> = systems.iter().map(|system| system.name.as_str()).collect();
    download_redump_batch(
        connection,
        progress_bar,
        base_url,
        &system_names,
        force,
        save_directory,
    )
    .await
}

/// The CLI batch: the run still fails at the end, naming the failed systems.
async fn download_redump_batch(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    base_url: &str,
    system_names: &[&str],
    force: bool,
    save_directory: Option<&str>,
) -> Result<()> {
    let outcomes = collect_redump_dats(
        connection,
        progress_bar,
        base_url,
        system_names,
        force,
        save_directory,
    )
    .await;
    if !outcomes.failed.is_empty() {
        let failed: Vec<&str> = outcomes
            .failed
            .iter()
            .map(|(system_name, _)| system_name.as_str())
            .collect();
        bail!("Failed to download: {}", failed.join(", "));
    }
    Ok(())
}

/// What a batch did, per system: each system lands in exactly one list.
#[derive(Debug, Default)]
pub struct DatOutcomes {
    pub imported: Vec<String>,
    pub up_to_date: Vec<String>,
    /// The system, and the error that stopped it.
    pub failed: Vec<(String, anyhow::Error)>,
}

/// Download and import each system in turn. A system that fails is reported
/// and skipped, so one broken DAT does not cost the rest of the batch. The CLI,
/// GraphQL and MCP all go through this one loop.
async fn collect_redump_dats(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    base_url: &str,
    system_names: &[impl AsRef<str>],
    force: bool,
    save_directory: Option<&str>,
) -> DatOutcomes {
    let mut outcomes = DatOutcomes::default();
    for system_name in system_names {
        let system_name = system_name.as_ref();
        match download_redump_dat(
            connection,
            progress_bar,
            base_url,
            system_name,
            force,
            save_directory,
        )
        .await
        {
            Ok(DatOutcome::Imported) => outcomes.imported.push(system_name.to_owned()),
            Ok(DatOutcome::UpToDate) => outcomes.up_to_date.push(system_name.to_owned()),
            Err(e) => {
                print_error(progress_bar, &format!("{e:#}"));
                outcomes.failed.push((system_name.to_owned(), e));
            }
        }
    }
    outcomes
}

/// What downloading one Redump system's DAT did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatOutcome {
    /// The DAT was new, or newer than the one in the database, and was imported.
    Imported,
    /// The database already holds this version or a newer one: nothing changed.
    UpToDate,
}

async fn download_redump_dat(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    base_url: &str,
    system_name: &str,
    force: bool,
    save_directory: Option<&str>,
) -> Result<DatOutcome> {
    print_header(progress_bar, &format!("Processing \"{}\"", system_name));
    let outcome = fetch_redump_dat(
        connection,
        progress_bar,
        base_url,
        system_name,
        force,
        save_directory,
    )
    .await;
    // rate limit, failures included
    sleep(Duration::from_secs(1)).await;
    print_separator(progress_bar);
    outcome
}

async fn fetch_redump_dat(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    base_url: &str,
    system_name: &str,
    force: bool,
    save_directory: Option<&str>,
) -> Result<DatOutcome> {
    let Some(code) = REDUMP_SYSTEMS_CODES.get(system_name) else {
        bail!("Unknown Redump system \"{}\"", system_name);
    };
    let zip_url = format!("{}/datfile/{}", base_url, code);
    let response = reqwest::get(&zip_url)
        .await
        .and_then(|response| response.error_for_status())
        .with_context(|| format!("Failed to download \"{zip_url}\""))?
        .bytes()
        .await
        .with_context(|| format!("Failed to read \"{zip_url}\""))?;
    let tmp_directory = create_tmp_directory(connection).await?;
    let mut zip_archive =
        ZipArchive::new(Cursor::new(response)).context("Failed to read Redump ZIP")?;
    if zip_archive.len() != 1 {
        bail!(
            "Expected one DAT file in the Redump ZIP, found {}",
            zip_archive.len()
        );
    }
    zip_archive
        .extract(&tmp_directory)
        .context("Failed to extract ZIP")?;
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

    let (datfile_xml, detector_xml) = parse_dat(progress_bar, &dat_file_path, true).await?;
    let summary = import_dat(
        connection,
        progress_bar,
        &datfile_xml,
        &detector_xml,
        None,
        None,
        force,
        false,
    )
    .await?;
    Ok(match summary {
        Some(_) => DatOutcome::Imported,
        None => DatOutcome::UpToDate,
    })
}

#[cfg(test)]
mod test_nointro;
#[cfg(test)]
mod test_nointro_failures;
#[cfg(test)]
mod test_redump;
#[cfg(test)]
mod test_redump_failures;

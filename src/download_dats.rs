use super::import_dats::import_dat;
use super::model::*;
use super::prompt::*;
use super::util::*;
use super::SimpleResult;
use clap::{App, Arg, ArgMatches, SubCommand};
use dialoguer::MultiSelect;
use indicatif::ProgressBar;
use phf::phf_map;
use quick_xml::de;
use rayon::iter::IntoParallelIterator;
use sqlx::SqliteConnection;
use std::io::Cursor;
use surf;
use zip::read::ZipArchive;

static NOINTRO_PROFILE_URL: &'static str = "https://datomatic.no-intro.org/profile.xml";

static NOINTRO_SYSTEM_URL: &'static str = "www.no-intro.org/";
static REDUMP_SYSTEM_URL: &'static str = "http://redump.org/";

static REDUMP_SYSTEMS_CODES: phf::Map<&'static str, &'static str> = phf_map! {
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
    "NEC - PC Engine CD & TurboGrafx CD" => "pce",
    "NEC - PC-88 series" => "pc-88",
    "NEC - PC-98 series" => "pc-98",
    "NEC - PC-FX & PC-FXGA" => "pc-fx",
    "Nintendo - GameCube" => "gc",
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
    "Sony - PlayStation Portable" => "psp",
    "TAB-Austria - Quizard" => "quizard",
    "Tomy - Kiss-Site" => "ksite",
    "VM Labs - NUON" => "nuon",
    "VTech - V.Flash & V.Smile Pro" => "vflash",
    "ZAPiT Games - Game Wave Family Entertainment System" => "gamewave",
};

pub fn subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name("download-dats")
        .about("Updates No-Intro and Redump DAT files")
        .arg(
            Arg::with_name("NOINTRO")
                .short("n")
                .long("nointro")
                .help("Downloads No-Intro DAT files")
                .required(false)
                .conflicts_with("REDUMP")
                .required_unless("REDUMP"),
        )
        .arg(
            Arg::with_name("REDUMP")
                .short("r")
                .long("redump")
                .help("Downloads Redump DAT files")
                .required(false)
                .conflicts_with("NOINTRO")
                .required_unless("NOINTRO"),
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
    if matches.is_present("NOINTRO") {
        download_nointro_dats(connection, progress_bar, matches.is_present("FORCE")).await?
    } else if matches.is_present("REDUMP") {
        download_redump_dats(connection, progress_bar, matches.is_present("FORCE")).await?
    }
    Ok(())
}

async fn download_nointro_dats(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    force: bool,
) -> SimpleResult<()> {
    let response = surf::get(NOINTRO_PROFILE_URL)
        .recv_string()
        .await
        .expect("Failed to download No-Intro profiles");
    let profile: ProfileXml = de::from_str(&response).expect("Failed to parse profile");
    let mut items: Vec<String> = profile
        .systems
        .into_iter()
        .map(|system| system.name)
        .collect();
    items.sort();
    let indices: Vec<usize> = try_with!(
        MultiSelect::new().paged(true).items(&items).interact(),
        "Failed to prompt for system(s)"
    );
    Ok(())
}

async fn download_redump_dats(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    force: bool,
) -> SimpleResult<()> {
    let mut items: Vec<&&str> = REDUMP_SYSTEMS_CODES.keys().collect();
    items.sort();
    let indices: Vec<usize> = try_with!(
        MultiSelect::new().paged(true).items(&items).interact(),
        "Failed to prompt for system(s)"
    );
    for i in indices {
        let code = *REDUMP_SYSTEMS_CODES.get(*items.get(i).unwrap()).unwrap();
        let zip_url = format!("{}datfile/{}/", REDUMP_SYSTEM_URL, code);
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

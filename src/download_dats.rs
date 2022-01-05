use super::database::*;
use super::import_dats::{import_dat, parse_dat};
use super::model::*;
use super::prompt::*;
use super::util::*;
use super::SimpleResult;
use async_std::task;
use cfg_if::cfg_if;
use clap::{App, Arg, ArgMatches};
use indicatif::ProgressBar;
use phf::phf_map;
use quick_xml::de;
use rayon::prelude::*;
use sqlx::sqlite::SqliteConnection;
use std::collections::HashSet;
use std::io::Cursor;
use std::time::Duration;
use zip::read::ZipArchive;

const NOINTRO_BASE_URL: &str = "https://datomatic.no-intro.org";
const NOINTRO_PROFILE_URL: &str = "/profile.xml";
const REDUMP_BASE_URL: &str = "http://redump.org";

const NOINTRO_SYSTEM_URL: &str = "www.no-intro.org";
const REDUMP_SYSTEM_URL: &str = "http://redump.org/";

cfg_if! {
    if #[cfg(test)] {
        static REDUMP_SYSTEMS_CODES: phf::Map<&str, &str> = phf_map! {
            "Test System" => "ts"
        };
    } else {
        static REDUMP_SYSTEMS_CODES: phf::Map<&str, &str> = phf_map! {
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
    }
}

pub fn subcommand<'a>() -> App<'a> {
    App::new("download-dats")
        .about("Download No-Intro and Redump DAT files and import them into oxyromon")
        .arg(
            Arg::new("NOINTRO")
                .short('n')
                .long("nointro")
                .help("Download No-Intro DAT files")
                .required(false)
                .conflicts_with("REDUMP")
                .required_unless_present("REDUMP"),
        )
        .arg(
            Arg::new("REDUMP")
                .short('r')
                .long("redump")
                .help("Download Redump DAT files")
                .required(false)
                .conflicts_with("NOINTRO")
                .required_unless_present("NOINTRO"),
        )
        .arg(
            Arg::new("UPDATE")
                .short('u')
                .long("update")
                .help("Check for system updates")
                .required(false),
        )
        .arg(
            Arg::new("ALL")
                .short('a')
                .long("all")
                .help("Import all systems")
                .required(false),
        )
        .arg(
            Arg::new("FORCE")
                .short('f')
                .long("force")
                .help("Force import of outdated DAT files")
                .required(false),
        )
}

pub async fn main(
    connection: &mut SqliteConnection,
    matches: &ArgMatches,
    progress_bar: &ProgressBar,
) -> SimpleResult<()> {
    if matches.is_present("NOINTRO") {
        if matches.is_present("UPDATE") {
            update_nointro_dats(
                connection,
                progress_bar,
                NOINTRO_BASE_URL,
                matches.is_present("ALL"),
            )
            .await?
        } else {
            progress_bar.println("Not supported");
        }
    } else if matches.is_present("REDUMP") {
        if matches.is_present("UPDATE") {
            update_redump_dats(
                connection,
                progress_bar,
                REDUMP_BASE_URL,
                matches.is_present("ALL"),
                matches.is_present("FORCE"),
            )
            .await?
        } else {
            download_redump_dats(
                connection,
                progress_bar,
                REDUMP_BASE_URL,
                matches.is_present("ALL"),
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
    let response = surf::get(format!("{}{}", base_url, NOINTRO_PROFILE_URL))
        .recv_string()
        .await
        .expect("Failed to download No-Intro profiles");
    let profile: ProfileXml = try_with!(de::from_str(&response), "Failed to parse profile");
    let systems = prompt_for_systems(connection, Some(NOINTRO_SYSTEM_URL), false, all).await?;
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
    Ok(())
}

async fn download_redump_dats(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    base_url: &str,
    all: bool,
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
        multiselect(&items, None)?
    };
    for i in indices {
        download_redump_dat(
            connection,
            progress_bar,
            base_url,
            items.get(i).unwrap(),
            false,
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
) -> SimpleResult<()> {
    let systems = prompt_for_systems(connection, Some(REDUMP_SYSTEM_URL), false, all).await?;
    for system in systems {
        download_redump_dat(connection, progress_bar, base_url, &system.name, force).await?;
    }
    Ok(())
}

async fn download_redump_dat(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    base_url: &str,
    system_name: &str,
    force: bool,
) -> SimpleResult<()> {
    progress_bar.println(format!("Processing \"{}\"", system_name));
    let code = *REDUMP_SYSTEMS_CODES.get(system_name).unwrap();
    let zip_url = format!("{}/datfile/{}/", base_url, code);
    let response = surf::get(zip_url)
        .recv_bytes()
        .await
        .expect("Failed to download ZIP");
    let tmp_directory = create_tmp_directory(connection).await?;
    let mut zip_archive = try_with!(ZipArchive::new(Cursor::new(response)), "Failed to read ZIP");
    match zip_archive.len() {
        0 => progress_bar.println("Update ZIP is empty"),
        1 => {
            try_with!(zip_archive.extract(&tmp_directory), "Failed to extract ZIP");
            let (datfile_xml, detector_xml) = parse_dat(
                progress_bar,
                &tmp_directory
                    .path()
                    .join(zip_archive.file_names().next().unwrap()),
                true,
            )
            .await?;
            import_dat(
                connection,
                progress_bar,
                &datfile_xml,
                &detector_xml,
                false,
                force,
            )
            .await?;
        }
        _ => progress_bar.println("Update ZIP contains too many files"),
    }
    // rate limit
    task::sleep(Duration::from_secs(1)).await;
    progress_bar.println("");
    Ok(())
}

#[cfg(test)]
mod test {
    extern crate wiremock;

    use super::super::config::*;
    use super::super::database::*;
    use super::super::import_dats;
    use super::super::util::*;
    use super::*;
    use async_std::fs;
    use async_std::io::prelude::*;
    use async_std::path::{Path, PathBuf};
    use tempfile::{NamedTempFile, TempDir};
    use wiremock::matchers::{method, path, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[async_std::test]
    async fn test_download_nointro_dat() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        set_tmp_directory(PathBuf::from(tmp_directory.path()));

        let matches = import_dats::subcommand()
            .get_matches_from(&["import-dats", "test/Test System (20200721).dat"]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let profile_xml_path = test_directory.join("profile.xml");

        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/profile.xml"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(fs::read_to_string(&profile_xml_path).await.unwrap()),
            )
            .mount(&mock_server)
            .await;

        // when
        update_nointro_dats(&mut connection, &progress_bar, &mock_server.uri(), true)
            .await
            .unwrap();

        // then
        //do nothing
    }

    #[async_std::test]
    async fn test_download_redump_dat() {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        set_tmp_directory(PathBuf::from(tmp_directory.path()));

        let zip_path = test_directory.join("Test System (20200721).zip");
        let mut zip_data = Vec::new();
        open_file(&zip_path)
            .await
            .unwrap()
            .read_to_end(&mut zip_data)
            .await
            .unwrap();

        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/datfile/[a-z0-9-]+/$"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(zip_data))
            .mount(&mock_server)
            .await;

        // when
        download_redump_dats(&mut connection, &progress_bar, &mock_server.uri(), true)
            .await
            .unwrap();

        // then
        let systems = find_systems(&mut connection).await;
        assert_eq!(systems.len(), 1);

        let system = systems.get(0).unwrap();
        assert_eq!(system.name, "Test System");

        assert_eq!(find_games(&mut connection).await.len(), 6);
        assert_eq!(find_roms(&mut connection).await.len(), 8);
    }
}

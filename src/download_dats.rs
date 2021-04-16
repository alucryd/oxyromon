use super::import_dats::import_dat;
use super::model::*;
use super::prompt::*;
use super::util::*;
use super::SimpleResult;
use clap::{App, Arg, ArgMatches, SubCommand};
use indicatif::ProgressBar;
use phf::phf_map;
use quick_xml::de;
use sqlx::SqliteConnection;
use std::io::Cursor;
use surf;
use zip::read::ZipArchive;

static NOINTRO_BASE_URL: &'static str = "https://datomatic.no-intro.org";
static NOINTRO_PROFILE_URL: &'static str = "/profile.xml";
static REDUMP_BASE_URL: &'static str = "http://redump.org";

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
        .about("Downloads No-Intro and Redump DAT files and imports them into oxyromon")
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
            Arg::with_name("ALL")
                .short("a")
                .long("all")
                .help("Imports all systems")
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
    if matches.is_present("NOINTRO") {
        progress_bar.println("Not supported");
        return Ok(());
        download_nointro_dats(
            connection,
            progress_bar,
            NOINTRO_BASE_URL,
            matches.is_present("ALL"),
            matches.is_present("FORCE"),
        )
        .await?
    } else if matches.is_present("REDUMP") {
        download_redump_dats(
            connection,
            progress_bar,
            REDUMP_BASE_URL,
            matches.is_present("ALL"),
            matches.is_present("FORCE"),
        )
        .await?
    }
    Ok(())
}

async fn download_nointro_dats(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    base_url: &str,
    all: bool,
    force: bool,
) -> SimpleResult<()> {
    let response = surf::get(format!("{}{}", base_url, NOINTRO_PROFILE_URL))
        .recv_string()
        .await
        .expect("Failed to download No-Intro profiles");
    let profile: ProfileXml = de::from_str(&response).expect("Failed to parse profile");
    let mut items: Vec<&str> = profile
        .systems
        .iter()
        .map(|system| system.name.as_str())
        .collect();
    items.sort();
    let indices: Vec<usize> = if all {
        (0..items.len()).collect()
    } else {
        multiselect(&items, None)?
    };
    Ok(())
}

async fn download_redump_dats(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    base_url: &str,
    all: bool,
    force: bool,
) -> SimpleResult<()> {
    let mut items: Vec<&str> = REDUMP_SYSTEMS_CODES.keys().map(|s| *s).collect();
    items.sort();
    let indices: Vec<usize> = if all {
        (0..items.len()).collect()
    } else {
        multiselect(&items, None)?
    };
    for i in indices {
        let code = *REDUMP_SYSTEMS_CODES.get(*items.get(i).unwrap()).unwrap();
        let zip_url = format!("{}/datfile/{}/", base_url, code);
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
    use super::super::config::*;
    use super::super::database::*;
    use super::super::util::*;
    use super::*;
    use async_std::fs;
    use async_std::io::prelude::*;
    use async_std::path::{Path, PathBuf};
    use async_std::sync::Mutex;
    use tempfile::{NamedTempFile, TempDir};
    use wiremock::matchers::{method, path, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[async_std::test]
    async fn test_download_nointro_dat() {
        // given
        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

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
        download_nointro_dats(
            &mut connection,
            &progress_bar,
            &mock_server.uri(),
            true,
            false,
        )
        .await
        .unwrap();
    }

    #[async_std::test]
    async fn test_download_redump_dat() {
        // given
        let _guard = MUTEX.get_or_init(|| Mutex::new(0)).lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let mut connection = establish_connection(db_file.path().to_str().unwrap()).await;

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
        download_redump_dats(
            &mut connection,
            &progress_bar,
            &mock_server.uri(),
            true,
            false,
        )
        .await
        .unwrap();

        // then
        let mut systems = find_systems(&mut connection).await;
        assert_eq!(systems.len(), 1);

        let system = systems.remove(0);
        assert_eq!(system.name, "Test System");

        assert_eq!(find_games(&mut connection).await.len(), 6);
        assert_eq!(find_roms(&mut connection).await.len(), 8);
    }
}

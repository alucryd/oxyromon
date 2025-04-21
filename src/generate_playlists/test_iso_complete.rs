use super::super::database::*;
use super::super::import_dats;
use super::super::import_roms;
use super::*;
use std::env;
use std::path::Path;
use tempfile::{NamedTempFile, TempDir};
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    // TODO: Audit that the environment access only happens in single-threaded code.
    unsafe {
        env::set_var(
            "PATH",
            format!(
                "{}:{}",
                test_directory.as_os_str().to_str().unwrap(),
                env::var("PATH").unwrap()
            ),
        )
    };
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(&test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(&test_directory).unwrap();
    let tmp_directory =
        set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand().get_matches_from(&[
        "import-dats",
        "tests/Test System (20230105) (Multiple Discs).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();

    for i in 1..=2 {
        let romfile_name = format!("Test Game (USA, Europe) (Disc {}).iso", i);
        let romfile_path = tmp_directory.join(&romfile_name);
        fs::copy(test_directory.join(&romfile_name), &romfile_path)
            .await
            .unwrap();
        let matches = import_roms::subcommand()
            .get_matches_from(&["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
        import_roms::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();
    }

    // when
    process_system(&mut connection, &progress_bar, &system)
        .await
        .unwrap();

    // then
    let games = find_full_games_by_system_id(&mut connection, system.id).await;
    assert_eq!(games.len(), 2);

    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), 2);

    let playlist_id = games.first().unwrap().playlist_id;
    assert!(playlist_id.is_some());

    let playlist = find_romfile_by_id(&mut connection, playlist_id.unwrap()).await;
    let playlist_path = system_directory.join("Test Game (USA, Europe).m3u");
    assert_eq!(
        playlist.path,
        playlist_path
            .strip_prefix(&rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap()
    );
    assert!(playlist_path.is_file());
    assert_eq!(playlist.romfile_type, RomfileType::Playlist as i64);

    let lines = fs::read_to_string(playlist_path)
        .await
        .unwrap()
        .split("\n")
        .map(|s| s.to_owned())
        .collect::<Vec<String>>();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines.first().unwrap(), &roms.first().unwrap().name);
    assert_eq!(lines.get(1).unwrap(), &roms.get(1).unwrap().name);
    assert_eq!(lines.get(2).unwrap(), "");
}

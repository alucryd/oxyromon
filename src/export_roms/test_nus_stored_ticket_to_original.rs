use super::super::import_dats;
use super::super::import_roms;
use super::*;
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};

#[tokio::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    let tmp_directory =
        set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand().get_matches_from([
        "import-dats",
        "tests/Test System (20261025) (CDN No Ticket).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();
    let update_directory = tmp_directory.join("Test Game (USA) (Update)");
    std::fs::create_dir(&update_directory).unwrap();
    // with the ticket a downloader made, which the DAT does not list
    for entry in std::fs::read_dir(test_directory.join("Test Game (USA) (Update)")).unwrap() {
        let path = entry.unwrap().path();
        let name = match path.file_name().unwrap().to_str().unwrap() {
            "cetk.48" => "title.tik",
            name => name,
        };
        std::fs::copy(&path, update_directory.join(name)).unwrap();
    }
    let matches = import_roms::subcommand()
        .get_matches_from(["import-roms", update_directory.to_str().unwrap()]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    let games = find_full_games_by_system_id(&mut connection, system.id).await;
    let (roms_by_game_id, games_by_id, romfiles_by_id) =
        load_rom_data(&mut connection, games, false).await;

    let destination_directory = tmp_directory.join("destination");
    create_directory(&progress_bar, &destination_directory, true)
        .await
        .unwrap();

    // when
    to_original(
        &mut connection,
        &progress_bar,
        &destination_directory,
        &system,
        games_by_id,
        roms_by_game_id,
        romfiles_by_id,
    )
    .await
    .unwrap();

    // then the stored ticket goes with the title, as WUP Installer needs it
    let game_directory = destination_directory.join("Test Game (USA) (Update)");
    assert_eq!(
        std::fs::read(game_directory.join("title.tik")).unwrap(),
        std::fs::read(
            test_directory
                .join("Test Game (USA) (Update)")
                .join("cetk.48")
        )
        .unwrap()
    );
    for name in ["00000010", "00000011", "00000012", "00000012.h3", "tmd.48"] {
        assert!(game_directory.join(name).is_file(), "{name}");
    }
}

use super::super::config::*;
use super::super::import_dats;
use super::super::import_roms;
use super::*;
use tempfile::{NamedTempFile, TempDir};
use tokio::fs;

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

    let matches = import_dats::subcommand()
        .get_matches_from(["import-dats", "tests/Test System (20200721).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let romfile_name = "Test Game (USA, Europe).rom";
    let romfile_path = tmp_directory.join(romfile_name);
    fs::copy(test_directory.join(romfile_name), &romfile_path)
        .await
        .unwrap();
    let matches = import_roms::subcommand()
        .get_matches_from(["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // a file of the same name already waits in the Trash
    let system = find_systems(&mut connection).await.remove(0);
    let trash_directory = get_trash_directory(&mut connection, Some(&system))
        .await
        .unwrap();
    fs::create_dir_all(&trash_directory).await.unwrap();
    fs::write(trash_directory.join(romfile_name), b"already trashed")
        .await
        .unwrap();
    let game = find_games_by_system_id(&mut connection, system.id)
        .await
        .into_iter()
        .find(|game| game.name == "Test Game (USA, Europe)")
        .unwrap();

    // when
    purge_games(&mut connection, &progress_bar, std::slice::from_ref(&game))
        .await
        .unwrap();

    // then neither file is touched, and the game keeps its file
    assert_eq!(
        fs::read(trash_directory.join(romfile_name)).await.unwrap(),
        b"already trashed"
    );
    let roms = find_roms_with_romfile_by_game_id(&mut connection, game.id).await;
    assert_eq!(roms.len(), 1);
    let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap()).await;
    assert!(!romfile.path.contains("Trash"));
    assert!(
        romfile
            .as_common(&mut connection)
            .await
            .unwrap()
            .path
            .is_file()
    );
}

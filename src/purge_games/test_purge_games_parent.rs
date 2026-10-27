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

    for romfile_name in ["Test Game (USA, Europe).rom", "Test Game (Japan).rom"] {
        let romfile_path = tmp_directory.join(romfile_name);
        fs::copy(test_directory.join(romfile_name), &romfile_path)
            .await
            .unwrap();
        let matches = import_roms::subcommand()
            .get_matches_from(["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
        import_roms::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();
    }

    // the Japanese file depends on the American one, as a child CHD on its parent
    let system = find_systems(&mut connection).await.remove(0);
    let games = find_games_by_system_id(&mut connection, system.id).await;
    let parent_game = games
        .iter()
        .find(|game| game.name == "Test Game (USA, Europe)")
        .unwrap();
    let child_game = games
        .iter()
        .find(|game| game.name == "Test Game (Japan)")
        .unwrap();
    let parent_romfile_id = find_roms_with_romfile_by_game_id(&mut connection, parent_game.id)
        .await
        .remove(0)
        .romfile_id
        .unwrap();
    let child_romfile_id = find_roms_with_romfile_by_game_id(&mut connection, child_game.id)
        .await
        .remove(0)
        .romfile_id
        .unwrap();
    update_romfile_parent(&mut connection, child_romfile_id, Some(parent_romfile_id)).await;

    // when the parent's game is purged alone
    purge_games(
        &mut connection,
        &progress_bar,
        std::slice::from_ref(parent_game),
    )
    .await
    .unwrap();

    // then its file stays for the child
    let parent_romfile = find_romfile_by_id(&mut connection, parent_romfile_id).await;
    assert!(!parent_romfile.path.contains("Trash"));
    assert_eq!(
        find_roms_by_romfile_id(&mut connection, parent_romfile_id)
            .await
            .len(),
        1
    );

    // when both games are purged
    purge_games(
        &mut connection,
        &progress_bar,
        &[parent_game.clone(), child_game.clone()],
    )
    .await
    .unwrap();

    // then both files go
    for romfile_id in [parent_romfile_id, child_romfile_id] {
        let romfile = find_romfile_by_id(&mut connection, romfile_id).await;
        assert!(romfile.path.contains("Trash"), "{}", romfile.path);
        assert!(
            find_roms_by_romfile_id(&mut connection, romfile_id)
                .await
                .is_empty()
        );
    }
}

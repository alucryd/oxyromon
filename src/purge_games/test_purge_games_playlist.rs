use super::super::config::*;
use super::super::generate_playlists;
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

    let matches = import_dats::subcommand().get_matches_from([
        "import-dats",
        "tests/Test System (20230105) (Multiple Discs).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    for i in 1..=2 {
        let romfile_name = format!("Test Game (USA, Europe) (Disc {}).iso", i);
        let romfile_path = tmp_directory.join(&romfile_name);
        fs::copy(test_directory.join(&romfile_name), &romfile_path)
            .await
            .unwrap();
        let matches = import_roms::subcommand()
            .get_matches_from(["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
        import_roms::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();
    }

    let matches = generate_playlists::subcommand().get_matches_from(["generate-playlists", "-a"]);
    generate_playlists::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    let games = find_games_by_system_id(&mut connection, system.id).await;
    let playlist_id = games[0].playlist_id.unwrap();
    assert_eq!(games[1].playlist_id, Some(playlist_id));

    // when one disc is purged
    purge_games(&mut connection, &progress_bar, &games[..1])
        .await
        .unwrap();

    // then the playlist stays, still listed by both discs
    let playlist = find_romfile_by_id(&mut connection, playlist_id).await;
    assert!(!playlist.path.contains("Trash"));
    let games = find_games_by_system_id(&mut connection, system.id).await;
    assert!(
        games
            .iter()
            .all(|game| game.playlist_id == Some(playlist_id))
    );

    // when the other disc is purged
    purge_games(&mut connection, &progress_bar, &games[1..])
        .await
        .unwrap();

    // then the playlist joins the discs in the Trash, and no game lists it
    let playlist = find_romfile_by_id(&mut connection, playlist_id).await;
    assert!(playlist.path.contains("Trash"));
    assert!(
        playlist
            .as_common(&mut connection)
            .await
            .unwrap()
            .path
            .is_file()
    );
    let games = find_games_by_system_id(&mut connection, system.id).await;
    assert!(games.iter().all(|game| game.playlist_id.is_none()));
    assert_eq!(find_romfiles_in_trash(&mut connection).await.len(), 3);
}

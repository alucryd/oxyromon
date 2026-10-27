use super::super::config::*;
use super::super::import_dats;
use super::super::import_roms;
use super::super::sort_roms;
use super::*;
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

    let matches = import_dats::subcommand()
        .get_matches_from(["import-dats", "tests/Test System (20261025) (CDN).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let game_name = "Test Game (USA) (Update)";
    let nus_directory = tmp_directory.join(game_name);
    std::fs::create_dir(&nus_directory).unwrap();
    for entry in std::fs::read_dir(test_directory.join(game_name)).unwrap() {
        let path = entry.unwrap().path();
        std::fs::copy(&path, nus_directory.join(path.file_name().unwrap())).unwrap();
    }
    let matches = import_roms::subcommand()
        .get_matches_from(["import-roms", tmp_directory.to_str().unwrap()]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();
    let game = find_games_by_system_id(&mut connection, system.id)
        .await
        .remove(0);
    assert_eq!(game.completion, 2);

    // when
    purge_games(&mut connection, &progress_bar, std::slice::from_ref(&game))
        .await
        .unwrap();
    let matches = sort_roms::subcommand().get_matches_from(["sort-roms", "-a", "-y"]);
    sort_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then the title's folder moved whole into the Trash, and its old place is gone
    let trash_directory = get_trash_directory(&mut connection, Some(&system))
        .await
        .unwrap();
    for name in [
        "00000010",
        "00000011",
        "00000012",
        "00000012.h3",
        "cetk.48",
        "tmd.48",
    ] {
        let path = trash_directory.join(game_name).join(name);
        assert!(path.is_file(), "{}", path.display());
    }
    assert!(!system_directory.join(game_name).exists());
    assert_eq!(find_romfiles_in_trash(&mut connection).await.len(), 6);
    let game = find_game_by_id(&mut connection, game.id).await;
    assert_eq!(game.completion, 0);
}

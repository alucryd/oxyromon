use super::super::config::*;
use super::super::import_dats;
use super::super::import_roms;
use super::super::sort_roms;
use super::*;
use std::path::PathBuf;
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

    let system = find_systems(&mut connection).await.remove(0);

    // when
    let matches = super::subcommand().get_matches_from([
        "purge-games",
        "-s",
        &system.name,
        "-g",
        "Test Game (USA, Europe)",
    ]);
    super::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // a later sort must not bring the file back
    let matches = sort_roms::subcommand().get_matches_from(["sort-roms", "-a", "-y"]);
    sort_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then
    let games = find_games_by_system_id(&mut connection, system.id).await;
    let purged = games
        .iter()
        .find(|game| game.name == "Test Game (USA, Europe)")
        .unwrap();
    assert_eq!(purged.completion, 0);
    assert!(
        find_roms_with_romfile_by_game_id(&mut connection, purged.id)
            .await
            .is_empty()
    );
    let kept = games
        .iter()
        .find(|game| game.name == "Test Game (Japan)")
        .unwrap();
    assert_eq!(kept.completion, 2);

    let trash_directory = get_trash_directory(&mut connection, Some(&system))
        .await
        .unwrap();
    assert!(
        trash_directory
            .join("Test Game (USA, Europe).rom")
            .is_file()
    );

    let romfiles = find_romfiles_in_trash(&mut connection).await;
    assert_eq!(romfiles.len(), 1);
    assert!(
        find_roms_by_romfile_id(&mut connection, romfiles[0].id)
            .await
            .is_empty()
    );
}

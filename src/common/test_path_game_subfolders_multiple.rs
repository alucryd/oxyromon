use super::super::import_dats;
use std::path::Path;
use tempfile::{NamedTempFile, TempDir};

use super::*;

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
    set_string(&mut connection, "GAME_SUBFOLDERS", "multiple", None).await;

    for dat in [
        "tests/Test System (20200721).dat",
        "tests/Test System (20230105) (Multiple Discs).dat",
    ] {
        let matches = import_dats::subcommand().get_matches_from(["import-dats", dat]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();
    }
    let systems = find_systems(&mut connection).await;
    let romfile = Romfile {
        id: 1,
        path: String::from("romfile.rom"),
        size: 0,
        parent_id: None,
        romfile_type: RomfileType::Romfile as i64,
    };

    // when / then
    let mut paths = Vec::new();
    for (game_name, extension) in [
        // several ROMs, stored as they are: a folder
        ("Test Game (USA, Europe) (CUE BIN)", None),
        // the same ROMs in one disc image: none
        ("Test Game (USA, Europe) (CUE BIN)", Some("chd")),
        // one ROM: none
        ("Test Game (Asia)", None),
        // a disc of a game of several, even as one image: the folder its discs share
        ("Test Game (USA, Europe) (Disc 1)", Some("chd")),
    ] {
        let mut found = None;
        for system in &systems {
            let games = find_games_by_system_id(&mut connection, system.id).await;
            if let Some(game) = games.into_iter().find(|game| game.name == game_name) {
                found = Some((system.id, game));
                break;
            }
        }
        let (system_id, game) = found.unwrap();
        let system = find_system_by_id(&mut connection, system_id).await;
        let rom = find_roms_by_game_id_no_parents(&mut connection, game.id)
            .await
            .remove(0);
        let path = romfile
            .as_common(&mut connection)
            .await
            .unwrap()
            .get_sorted_path(&mut connection, &system, &game, &rom, &None, &extension)
            .await
            .unwrap();
        let system_directory = get_system_directory(&mut connection, &system)
            .await
            .unwrap();
        paths.push(
            path.parent()
                .unwrap()
                .strip_prefix(&system_directory)
                .unwrap()
                .to_path_buf(),
        );
    }
    assert_eq!(
        paths,
        vec![
            PathBuf::from("Test Game (USA, Europe) (CUE BIN)"),
            PathBuf::new(),
            PathBuf::new(),
            PathBuf::from("Test Game (USA, Europe)"),
        ]
    );
}

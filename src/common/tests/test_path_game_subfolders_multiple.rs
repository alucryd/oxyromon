use crate::test_support::TestEnv;

use super::*;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    set_string(&mut env.connection, "GAME_SUBFOLDERS", "multiple", None).await;

    env.import_dats(&[
        "Test System (20200721).dat",
        "Test System (20230105) (Multiple Discs).dat",
    ])
    .await;
    let systems = find_systems(&mut env.connection).await;
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
            let games = find_games_by_system_id(&mut env.connection, system.id).await;
            if let Some(game) = games.into_iter().find(|game| game.name == game_name) {
                found = Some((system.id, game));
                break;
            }
        }
        let (system_id, game) = found.unwrap();
        let system = find_system_by_id(&mut env.connection, system_id).await;
        let rom = find_roms_by_game_id_no_parents(&mut env.connection, game.id)
            .await
            .remove(0);
        let path = romfile
            .as_common(&mut env.connection)
            .await
            .unwrap()
            .get_sorted_path(&mut env.connection, &system, &game, &rom, &None, &extension)
            .await
            .unwrap();
        let system_directory = get_system_directory(&mut env.connection, &system)
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

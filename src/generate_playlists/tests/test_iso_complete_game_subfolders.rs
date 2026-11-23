use crate::config::*;
use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    set_string(&mut env.connection, "GAME_SUBFOLDERS", "always", None).await;

    env.import_dats(&["Test System (20230105) (Multiple Discs).dat"])
        .await;

    let system = env.system().await;
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();

    for i in 1..=2 {
        let romfile_name = format!("Test Game (USA, Europe) (Disc {}).iso", i);
        let romfile_path = env.fixture(&romfile_name).await;
        env.import_roms(&[&romfile_path]).await;
    }

    // when
    process_system(&mut env.connection, &env.progress_bar, &system)
        .await
        .unwrap();

    // then
    let games = find_full_games_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(games.len(), 2);

    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 2);

    let playlist_id = games.first().unwrap().playlist_id;
    assert!(playlist_id.is_some());

    let playlist = find_romfile_by_id(&mut env.connection, playlist_id.unwrap()).await;
    let playlist_path = system_directory
        .join("Test Game (USA, Europe)")
        .join("Test Game (USA, Europe).m3u");
    assert_eq!(
        playlist.path,
        playlist_path
            .strip_prefix(&env.rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap()
    );
    assert!(playlist_path.is_file());
    assert_eq!(playlist.romfile_type, RomfileType::Playlist as i64);

    let lines = fs::read_to_string(&playlist_path)
        .await
        .unwrap()
        .split("\n")
        .map(|s| s.to_owned())
        .collect::<Vec<String>>();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines.first().unwrap(), &roms.first().unwrap().name);
    assert_eq!(lines.get(1).unwrap(), &roms.get(1).unwrap().name);
    assert_eq!(lines.get(2).unwrap(), "");

    // the discs sit beside the playlist that names them
    for line in &lines[..2] {
        assert!(playlist_path.parent().unwrap().join(line).is_file());
    }

    // and sort-roms would leave it where it is
    assert_eq!(
        games
            .first()
            .unwrap()
            .get_playlist_path(&mut env.connection, &system, &None)
            .await
            .unwrap(),
        playlist_path
    );
}

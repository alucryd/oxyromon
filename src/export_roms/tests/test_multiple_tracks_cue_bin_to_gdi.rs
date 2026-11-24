use super::*;
use crate::test_support::TestEnv;
use std::path::PathBuf;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    let mut romfile_paths: Vec<PathBuf> = vec![];
    let romfile_path = env
        .fixture("Test Game (USA, Europe) (Multiple Tracks).cue")
        .await;
    romfile_paths.push(romfile_path);
    let romfile_path = env
        .fixture("Test Game (USA, Europe) (CUE BIN) (Track 01).bin")
        .await;
    romfile_paths.push(romfile_path);
    let romfile_path = env
        .fixture("Test Game (USA, Europe) (CUE BIN) (Track 02).bin")
        .await;
    romfile_paths.push(romfile_path);

    let system = env.system().await;

    for romfile_path in romfile_paths {
        env.import_roms(&[&romfile_path]).await;
    }

    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    for rom in &roms {
        let romfile = find_romfile_by_id(&mut env.connection, rom.romfile_id.unwrap()).await;
        romfiles_by_id.insert(romfile.id, romfile);
    }
    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    roms_by_game_id.insert(roms[0].game_id, roms);

    let destination_directory = env.tmp_directory.join("destination");
    create_directory(&env.progress_bar, &destination_directory, true)
        .await
        .unwrap();

    // when
    to_gdi(
        &mut env.connection,
        &env.progress_bar,
        &destination_directory,
        roms_by_game_id,
        romfiles_by_id,
    )
    .await
    .unwrap();

    // then
    // What gdidrop writes for the same set, but for its " [gdidrop]" suffix:
    // track 1 past its 231-sector pregap, track 2 whole.
    assert_eq!(
        fs::read_to_string(destination_directory.join("Test Game (USA, Europe) (CUE BIN).gdi"))
            .await
            .unwrap(),
        "2\n\
         1 231 4 2352 \"Test Game (USA, Europe) (CUE BIN) (Track 01).bin\" 0\n\
         2 8635 0 2352 \"Test Game (USA, Europe) (CUE BIN) (Track 02).raw\" 0\n"
    );
    let track_01 = fs::metadata(
        destination_directory.join("Test Game (USA, Europe) (CUE BIN) (Track 01).bin"),
    )
    .await
    .unwrap();
    assert_eq!(track_01.len(), (8635 - 231) * 2352);
}

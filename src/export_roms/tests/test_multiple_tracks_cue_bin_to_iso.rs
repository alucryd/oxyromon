use super::*;
use crate::test_support::TestEnv;
use sha1::{Digest, Sha1};
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

    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    for rom in &roms {
        let romfile = find_romfile_by_id(&mut env.connection, rom.romfile_id.unwrap()).await;
        romfiles_by_id.insert(romfile.id, romfile);
    }
    roms_by_game_id.insert(roms[0].game_id, roms);

    let destination_directory = env.tmp_directory.join("destination");
    create_directory(&env.progress_bar, &destination_directory, true)
        .await
        .unwrap();

    // when
    to_iso(
        &mut env.connection,
        &env.progress_bar,
        &destination_directory,
        roms_by_game_id,
        romfiles_by_id,
    )
    .await
    .unwrap();

    // then
    let iso = fs::read(destination_directory.join("Test Game (USA, Europe) (CUE BIN).iso"))
        .await
        .unwrap();
    let sha1: String = Sha1::digest(&iso)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    // Only the first track, from the bin the cue names first: the same ISO the
    // single track set produces, since both describe that track identically.
    assert_eq!(iso.len(), 8404 * 2048);
    assert_eq!(sha1, "9ec402052624dbb7748235a6d9fb4299bba44848");
}

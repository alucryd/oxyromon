use super::*;
use crate::test_support::TestEnv;
use sha1::{Digest, Sha1};
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    // The cue and both bins, as the other archived multiple tracks tests use.
    let romfile_path = env.fixture("Test Game (USA, Europe) (Full).7z").await;

    let system = env.system().await;

    env.import_roms(&[&romfile_path]).await;

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
    // The first track alone, as the loose CUE/BIN set gives.
    assert_eq!(iso.len(), 8404 * 2048);
    assert_eq!(sha1, "9ec402052624dbb7748235a6d9fb4299bba44848");
}

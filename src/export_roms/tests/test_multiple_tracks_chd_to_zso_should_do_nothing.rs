use super::*;
use crate::test_support::TestEnv;
use crate::xso::XsoType;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    let cue_romfile_path = env
        .fixture("Test Game (USA, Europe) (Multiple Tracks).cue")
        .await;
    let chd_romfile_path = env
        .fixture("Test Game (USA, Europe) (Multiple Tracks).chd")
        .await;

    let system = env.system().await;

    env.run(&[
        "import-roms",
        chd_romfile_path.as_os_str().to_str().unwrap(),
        cue_romfile_path.as_os_str().to_str().unwrap(),
    ])
    .await
    .unwrap();

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
    to_xso(
        &mut env.connection,
        &env.progress_bar,
        &destination_directory,
        roms_by_game_id,
        romfiles_by_id,
        XsoType::Zso,
    )
    .await
    .unwrap();

    // then
    assert!(
        !destination_directory
            .join("Test Game (USA, Europe).zso")
            .is_file()
    );
}

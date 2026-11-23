use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    let romfile_path = env
        .tmp_directory
        .join("Test Game (USA, Europe) (Multiple Tracks).ore");
    ore_rs::compress(
        &env.fixture_source("Test Game (USA, Europe) (Multiple Tracks).cue"),
        &romfile_path,
        &ore_rs::Options::default(),
        &[],
        &mut |_| {},
    )
    .unwrap();

    let system = env.system().await;
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();

    // when
    env.run(&["import-roms", romfile_path.to_str().unwrap()])
        .await
        .unwrap();

    // then every ROM, the cue sheet's included, points to the ORE
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 3);
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);

    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe) (CUE BIN).ore")
            .strip_prefix(&env.rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(env.rom_directory.join(&romfile.path).is_file());
    assert!(roms.iter().all(|rom| rom.romfile_id == Some(romfile.id)));
    assert!(
        !system_directory
            .join("Test Game (USA, Europe) (CUE BIN).cue")
            .exists()
    );
}

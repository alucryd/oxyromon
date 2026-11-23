use super::*;
use crate::test_support::TestEnv;
use tempfile::TempDir;

#[tokio::test]
async fn test() {
    // given a title's ORE, made by one collection
    let elsewhere = TempDir::new().unwrap();
    let ore_path = elsewhere.path().join("Somewhere Else.ore");
    {
        let mut env = TestEnv::with_dat("Test System (20261025) (CDN).dat").await;
        let update_directory = env.fixture("Test Game (USA) (Update)").await;
        env.import_roms(&[&update_directory]).await;
        env.run(&["convert-roms", "-f", "ORE", "-a"]).await.unwrap();
        let system = env.system().await;
        let system_directory = env.system_directory(&system).await;
        std::fs::copy(
            system_directory.join("Test Game (USA) (Update).ore"),
            &ore_path,
        )
        .unwrap();
    }

    // and another collection that has only the DAT
    let mut env = TestEnv::with_dat("Test System (20261025) (CDN).dat").await;

    // when
    env.import_roms(&[&ore_path]).await;

    // then the ORE is the title's, under the game's name, every ROM in it
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;
    let ore = system_directory.join("Test Game (USA) (Update).ore");
    assert!(ore.is_file());
    assert!(!ore_path.exists());
    let romfiles = env.romfiles().await;
    assert_eq!(romfiles.len(), 1);
    assert_eq!(
        romfiles[0].path,
        ore.strip_prefix(&env.rom_directory)
            .unwrap()
            .to_str()
            .unwrap()
    );
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 6);
    assert!(
        roms.iter()
            .all(|rom| rom.romfile_id == Some(romfiles[0].id))
    );
}

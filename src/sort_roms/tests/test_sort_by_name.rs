use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    let romfile_names = vec![
        "Test Game (Asia).rom",
        "Test Game (Japan).rom",
        "Test Game (USA, Europe) (Beta).rom",
        "Test Game (USA, Europe).rom",
    ];
    for romfile_name in &romfile_names {
        let romfile_path = env.tmp_directory.join(romfile_name);
        fs::copy(env.fixture_source(romfile_name), &romfile_path)
            .await
            .unwrap();
        env.import_roms(&[&romfile_path]).await;
    }

    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;
    let expected_prefix = env.stored_path(&system_directory);

    // when
    let matches = subcommand().get_matches_from(["sort-roms", "-s", system.name.as_str(), "-y"]);
    main(&mut env.connection, &matches, &env.progress_bar)
        .await
        .unwrap();

    // then
    let romfiles = find_romfiles_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(4, romfiles.len());

    for romfile in &romfiles {
        assert!(romfile.path.starts_with(&expected_prefix));
        assert!(env.rom_directory.join(&romfile.path).is_file());
    }
}

use super::super::import_roms::{UnattendedMode, import_other};
use super::*;
use crate::test_support::TestEnv;
use std::collections::HashSet;

async fn import_file(env: &mut TestEnv, system: &System, fixture: &str, name: &str) {
    let path = env.tmp_directory.join(name);
    tokio::fs::copy(env.fixture_source(fixture), &path)
        .await
        .unwrap();
    let result = import_other(
        &mut env.connection,
        &env.progress_bar,
        &Some(system),
        &None,
        &HashSet::new(),
        CommonRomfile::from_path(&path).unwrap(),
        true,
        false,
        UnattendedMode::Skip,
    )
    .await
    .unwrap();
    assert!(result.is_some(), "Failed to match \"{}\"", name);
}

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721) (MAME Rebuild).dat").await;

    let system = find_arcade_systems(&mut env.connection).await.remove(0);
    let system_directory = env.system_directory(&system).await;

    // import BIOS and parent fully, clone only its exclusive rom
    import_file(&mut env, &system, "Test Game (Asia).rom", "Test BIOS.rom").await;
    import_file(
        &mut env,
        &system,
        "Test Game (USA, Europe).rom",
        "Test Parent.rom",
    )
    .await;
    import_file(
        &mut env,
        &system,
        "Test Game (Japan).rom",
        "Test Parent Common.rom",
    )
    .await;
    import_file(
        &mut env,
        &system,
        "Test Game (USA, Europe) (Beta).rom",
        "Test Clone Only.rom",
    )
    .await;

    let system = find_system_by_id(&mut env.connection, system.id).await;
    compute_system_completion(&mut env.connection, &env.progress_bar, &system)
        .await
        .unwrap();

    let clone = find_game_by_name_and_bios_and_system_id(
        &mut env.connection,
        "Test Clone",
        false,
        system.id,
    )
    .await
    .unwrap();

    // when rebuilding to FULL_NON_MERGED
    env.run(&["rebuild-roms", "--all", "-m", "FULL_NON_MERGED"])
        .await
        .unwrap();

    // then the clone got its own copies of the shared roms including the BIOS rom
    let system = find_system_by_id(&mut env.connection, system.id).await;
    assert_eq!(system.merging, Merging::FullNonMerged as i64);

    for name in [
        "Test Clone.rom",
        "Test Clone Common.rom",
        "Test Clone Bios.rom",
    ] {
        let rom = find_rom_by_name_and_game_id(&mut env.connection, name, clone.id)
            .await
            .unwrap();
        assert!(rom.romfile_id.is_some(), "\"{}\" was not expanded", name);
        let romfile = find_romfile_by_id(&mut env.connection, rom.romfile_id.unwrap()).await;
        let absolute = env.rom_directory.join(&romfile.path);
        assert!(absolute.is_file(), "Missing \"{:?}\"", absolute);
        assert!(absolute.starts_with(system_directory.join("Test Clone")));
    }

    // and the clone is now complete under FULL_NON_MERGED rules
    let clone = find_game_by_id(&mut env.connection, clone.id).await;
    assert_eq!(clone.completion, Completion::Full as i64);
}

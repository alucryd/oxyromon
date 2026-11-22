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

    // first expand to FULL_NON_MERGED so the clone owns a copy of the BIOS rom too
    env.run(&["rebuild-roms", "--all", "-m", "FULL_NON_MERGED"])
        .await
        .unwrap();
    let system = find_system_by_id(&mut env.connection, system.id).await;
    assert_eq!(system.merging, Merging::FullNonMerged as i64);

    let clone = find_game_by_name_and_bios_and_system_id(
        &mut env.connection,
        "Test Clone",
        false,
        system.id,
    )
    .await
    .unwrap();
    let clone_bios_rom =
        find_rom_by_name_and_game_id(&mut env.connection, "Test Clone Bios.rom", clone.id)
            .await
            .unwrap();
    assert!(clone_bios_rom.romfile_id.is_some());
    let clone_bios_path = env.rom_directory.join(
        find_romfile_by_id(&mut env.connection, clone_bios_rom.romfile_id.unwrap())
            .await
            .path,
    );
    assert!(clone_bios_path.is_file());

    // when trimming to NON_MERGED
    env.run(&["rebuild-roms", "--all", "-m", "NON_MERGED"])
        .await
        .unwrap();

    // then only the clone's BIOS rom was dropped
    let system = find_system_by_id(&mut env.connection, system.id).await;
    assert_eq!(system.merging, Merging::NonMerged as i64);

    let clone_bios_rom =
        find_rom_by_name_and_game_id(&mut env.connection, "Test Clone Bios.rom", clone.id)
            .await
            .unwrap();
    assert_eq!(clone_bios_rom.romfile_id, None);
    assert!(!clone_bios_path.is_file());

    // the parent-shared roms are kept
    for name in [
        "Test Clone.rom",
        "Test Clone Common.rom",
        "Test Clone Only.rom",
    ] {
        let rom = find_rom_by_name_and_game_id(&mut env.connection, name, clone.id)
            .await
            .unwrap();
        assert!(
            rom.romfile_id.is_some(),
            "\"{}\" should have been kept",
            name
        );
    }
}

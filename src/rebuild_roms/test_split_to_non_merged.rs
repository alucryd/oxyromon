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

    // import the parent game fully, the clone only its exclusive rom
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

    let mut system = find_system_by_id(&mut env.connection, system.id).await;
    compute_system_completion(&mut env.connection, &env.progress_bar, &system)
        .await
        .unwrap();
    system = find_system_by_id(&mut env.connection, system.id).await;

    let parent = find_game_by_name_and_bios_and_system_id(
        &mut env.connection,
        "Test Parent",
        false,
        system.id,
    )
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
    assert_eq!(parent.completion, Completion::Full as i64);
    // in Split mode a clone is full without its shared roms
    assert_eq!(clone.completion, Completion::Full as i64);
    let clone_bios_rom =
        find_rom_by_name_and_game_id(&mut env.connection, "Test Clone Bios.rom", clone.id)
            .await
            .unwrap();
    assert_eq!(clone_bios_rom.romfile_id, None);

    // when rebuilding to NON_MERGED
    env.run(&["rebuild-roms", "--all", "-m", "NON_MERGED"])
        .await
        .unwrap();

    // then the clone got its own copies of the shared roms
    let system = find_system_by_id(&mut env.connection, system.id).await;
    assert_eq!(system.merging, Merging::NonMerged as i64);

    let clone_rom = find_rom_by_name_and_game_id(&mut env.connection, "Test Clone.rom", clone.id)
        .await
        .unwrap();
    let clone_common_rom =
        find_rom_by_name_and_game_id(&mut env.connection, "Test Clone Common.rom", clone.id)
            .await
            .unwrap();
    assert!(clone_rom.romfile_id.is_some());
    assert!(clone_common_rom.romfile_id.is_some());
    // the BIOS rom is not required in NON_MERGED
    let clone_bios_rom =
        find_rom_by_name_and_game_id(&mut env.connection, "Test Clone Bios.rom", clone.id)
            .await
            .unwrap();
    assert_eq!(clone_bios_rom.romfile_id, None);

    // the copies live in the clone's game directory
    let clone_romfile =
        find_romfile_by_id(&mut env.connection, clone_rom.romfile_id.unwrap()).await;
    let clone_common_romfile =
        find_romfile_by_id(&mut env.connection, clone_common_rom.romfile_id.unwrap()).await;
    // named after the clone's roms, not the parent's they merge
    for (romfile, name) in [
        (&clone_romfile, "Test Clone.rom"),
        (&clone_common_romfile, "Test Clone Common.rom"),
    ] {
        let absolute = env.rom_directory.join(&romfile.path);
        assert!(absolute.is_file(), "Missing \"{:?}\"", absolute);
        assert_eq!(absolute, system_directory.join("Test Clone").join(name));
    }
    // the parent's own files are untouched
    let parent_rom =
        find_rom_by_name_and_game_id(&mut env.connection, "Test Parent.rom", parent.id)
            .await
            .unwrap();
    let parent_romfile =
        find_romfile_by_id(&mut env.connection, parent_rom.romfile_id.unwrap()).await;
    assert!(
        env.rom_directory
            .join(&parent_romfile.path)
            .starts_with(system_directory.join("Test Parent"))
    );
}

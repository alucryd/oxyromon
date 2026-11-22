use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721) (MAME Rebuild).dat").await;

    let system = find_arcade_systems(&mut env.connection).await.remove(0);
    let system_directory = env.system_directory(&system).await;

    // import the parent fully and the clone only its exclusive rom
    env.import_other(&system, "Test Game (USA, Europe).rom", "Test Parent.rom")
        .await;
    env.import_other(&system, "Test Game (Japan).rom", "Test Parent Common.rom")
        .await;
    env.import_other(
        &system,
        "Test Game (USA, Europe) (Beta).rom",
        "Test Clone Only.rom",
    )
    .await;

    let system = find_system_by_id(&mut env.connection, system.id).await;
    compute_system_completion(&mut env.connection, &env.progress_bar, &system)
        .await
        .unwrap();

    // first expand to NON_MERGED so the clone owns copies of the shared roms
    env.run(&["rebuild-roms", "--all", "-m", "NON_MERGED"])
        .await
        .unwrap();
    let system = find_system_by_id(&mut env.connection, system.id).await;
    assert_eq!(system.merging, Merging::NonMerged as i64);

    let clone = find_game_by_name_and_bios_and_system_id(
        &mut env.connection,
        "Test Clone",
        false,
        system.id,
    )
    .await
    .unwrap();
    let clone_rom = find_rom_by_name_and_game_id(&mut env.connection, "Test Clone.rom", clone.id)
        .await
        .unwrap();
    let clone_only_rom =
        find_rom_by_name_and_game_id(&mut env.connection, "Test Clone Only.rom", clone.id)
            .await
            .unwrap();
    assert!(clone_rom.romfile_id.is_some());
    let clone_rom_path = env.rom_directory.join(
        find_romfile_by_id(&mut env.connection, clone_rom.romfile_id.unwrap())
            .await
            .path,
    );
    let clone_only_path = env.rom_directory.join(
        find_romfile_by_id(&mut env.connection, clone_only_rom.romfile_id.unwrap())
            .await
            .path,
    );
    assert!(clone_rom_path.is_file());
    assert!(clone_only_path.is_file());

    // when trimming back to SPLIT
    env.run(&["rebuild-roms", "--all", "-m", "SPLIT"])
        .await
        .unwrap();

    // then the clone's shared roms lost their files, the exclusive one is kept
    let system = find_system_by_id(&mut env.connection, system.id).await;
    assert_eq!(system.merging, Merging::Split as i64);

    let clone_rom = find_rom_by_name_and_game_id(&mut env.connection, "Test Clone.rom", clone.id)
        .await
        .unwrap();
    let clone_common_rom =
        find_rom_by_name_and_game_id(&mut env.connection, "Test Clone Common.rom", clone.id)
            .await
            .unwrap();
    let clone_only_rom =
        find_rom_by_name_and_game_id(&mut env.connection, "Test Clone Only.rom", clone.id)
            .await
            .unwrap();
    assert_eq!(clone_rom.romfile_id, None);
    assert_eq!(clone_common_rom.romfile_id, None);
    assert!(clone_only_rom.romfile_id.is_some());
    assert!(!clone_rom_path.is_file());
    assert!(clone_only_path.is_file());

    // the parent's files are untouched
    let parent = find_game_by_name_and_bios_and_system_id(
        &mut env.connection,
        "Test Parent",
        false,
        system.id,
    )
    .await
    .unwrap();
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

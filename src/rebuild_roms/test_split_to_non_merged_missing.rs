use super::super::import_roms::{UnattendedMode, import_other};
use super::*;
use crate::test_support::TestEnv;
use std::collections::HashSet;
use tokio::fs;

async fn import_file(env: &mut TestEnv, system: &System, fixture: &str, name: &str) {
    let path = env.tmp_directory.join(name);
    fs::copy(env.fixture_source(fixture), &path).await.unwrap();
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

    // import the parent without its own rom, and the clone's exclusive rom
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

    // when rebuilding to NON_MERGED
    env.run(&["rebuild-roms", "--all", "-m", "NON_MERGED"])
        .await
        .unwrap();

    // then the clone, missing "Test Clone.rom", got none of the parent's roms
    let clone = find_game_by_name_and_bios_and_system_id(
        &mut env.connection,
        "Test Clone",
        false,
        system.id,
    )
    .await
    .unwrap();
    let clone_common_rom =
        find_rom_by_name_and_game_id(&mut env.connection, "Test Clone Common.rom", clone.id)
            .await
            .unwrap();
    assert_eq!(clone_common_rom.romfile_id, None);
    let mut entries = fs::read_dir(system_directory.join("Test Clone"))
        .await
        .unwrap();
    let mut names = vec![];
    while let Some(entry) = entries.next_entry().await.unwrap() {
        names.push(entry.file_name().into_string().unwrap());
    }
    assert_eq!(names, vec!["Test Clone Only.rom"]);
}

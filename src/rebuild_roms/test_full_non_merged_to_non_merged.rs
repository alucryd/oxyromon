use super::super::import_dats;
use super::super::import_roms::{UnattendedMode, import_other};
use super::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};
use tokio::fs;

async fn import_file(
    connection: &mut SqliteConnection,
    progress_bar: &ProgressBar,
    system: &System,
    tmp_directory: &Path,
    fixture: &str,
    name: &str,
) {
    let path = tmp_directory.join(name);
    fs::copy(Path::new("tests").join(fixture), &path)
        .await
        .unwrap();
    let result = import_other(
        connection,
        progress_bar,
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
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    let tmp_directory =
        set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand().get_matches_from([
        "import-dats",
        "tests/Test System (20200721) (MAME Rebuild).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_arcade_systems(&mut connection).await.remove(0);

    // import BIOS and parent fully, clone only its exclusive rom
    import_file(
        &mut connection,
        &progress_bar,
        &system,
        &tmp_directory,
        "Test Game (Asia).rom",
        "Test BIOS.rom",
    )
    .await;
    import_file(
        &mut connection,
        &progress_bar,
        &system,
        &tmp_directory,
        "Test Game (USA, Europe).rom",
        "Test Parent.rom",
    )
    .await;
    import_file(
        &mut connection,
        &progress_bar,
        &system,
        &tmp_directory,
        "Test Game (Japan).rom",
        "Test Parent Common.rom",
    )
    .await;
    import_file(
        &mut connection,
        &progress_bar,
        &system,
        &tmp_directory,
        "Test Game (USA, Europe) (Beta).rom",
        "Test Clone Only.rom",
    )
    .await;

    let system = find_system_by_id(&mut connection, system.id).await;
    compute_system_completion(&mut connection, &progress_bar, &system)
        .await
        .unwrap();

    // first expand to FULL_NON_MERGED so the clone owns a copy of the BIOS rom too
    let matches = subcommand().get_matches_from(["rebuild-roms", "--all", "-m", "FULL_NON_MERGED"]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();
    let system = find_system_by_id(&mut connection, system.id).await;
    assert_eq!(system.merging, Merging::FullNonMerged as i64);

    let clone =
        find_game_by_name_and_bios_and_system_id(&mut connection, "Test Clone", false, system.id)
            .await
            .unwrap();
    let clone_bios_rom =
        find_rom_by_name_and_game_id(&mut connection, "Test Clone Bios.rom", clone.id)
            .await
            .unwrap();
    assert!(clone_bios_rom.romfile_id.is_some());
    let clone_bios_path = rom_directory.path().join(
        find_romfile_by_id(&mut connection, clone_bios_rom.romfile_id.unwrap())
            .await
            .path,
    );
    assert!(clone_bios_path.is_file());

    // when trimming to NON_MERGED
    let matches = subcommand().get_matches_from(["rebuild-roms", "--all", "-m", "NON_MERGED"]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then only the clone's BIOS rom was dropped
    let system = find_system_by_id(&mut connection, system.id).await;
    assert_eq!(system.merging, Merging::NonMerged as i64);

    let clone_bios_rom =
        find_rom_by_name_and_game_id(&mut connection, "Test Clone Bios.rom", clone.id)
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
        let rom = find_rom_by_name_and_game_id(&mut connection, name, clone.id)
            .await
            .unwrap();
        assert!(
            rom.romfile_id.is_some(),
            "\"{}\" should have been kept",
            name
        );
    }
}

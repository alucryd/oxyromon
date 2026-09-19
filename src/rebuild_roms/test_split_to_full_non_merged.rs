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
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();

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

    let clone =
        find_game_by_name_and_bios_and_system_id(&mut connection, "Test Clone", false, system.id)
            .await
            .unwrap();

    // when rebuilding to FULL_NON_MERGED
    let matches = subcommand().get_matches_from(["rebuild-roms", "--all", "-m", "FULL_NON_MERGED"]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then the clone got its own copies of the shared roms including the BIOS rom
    let system = find_system_by_id(&mut connection, system.id).await;
    assert_eq!(system.merging, Merging::FullNonMerged as i64);

    for name in [
        "Test Clone.rom",
        "Test Clone Common.rom",
        "Test Clone Bios.rom",
    ] {
        let rom = find_rom_by_name_and_game_id(&mut connection, name, clone.id)
            .await
            .unwrap();
        assert!(rom.romfile_id.is_some(), "\"{}\" was not expanded", name);
        let romfile = find_romfile_by_id(&mut connection, rom.romfile_id.unwrap()).await;
        let absolute = rom_directory.path().join(&romfile.path);
        assert!(absolute.is_file(), "Missing \"{:?}\"", absolute);
        assert!(absolute.starts_with(system_directory.join("Test Clone")));
    }

    // and the clone is now complete under FULL_NON_MERGED rules
    let clone = find_game_by_id(&mut connection, clone.id).await;
    assert_eq!(clone.completion, Completion::Full as i64);
}

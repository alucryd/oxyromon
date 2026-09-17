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
    tmp_directory: &PathBuf,
    fixture: &str,
    name: &str,
) {
    let path = tmp_directory.join(name);
    fs::copy(Path::new("tests").join(fixture), &path).await.unwrap();
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

    let matches = import_dats::subcommand()
        .get_matches_from(["import-dats", "tests/Test System (20200721) (MAME Rebuild).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_arcade_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system).await.unwrap();

    // import the parent fully and the clone only its exclusive rom
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

    // first expand to NON_MERGED so the clone owns copies of the shared roms
    let matches = subcommand().get_matches_from(["rebuild-roms", "--all", "-m", "NON_MERGED"]);
    main(&mut connection, &matches, &progress_bar).await.unwrap();
    let system = find_system_by_id(&mut connection, system.id).await;
    assert_eq!(system.merging, Merging::NonMerged as i64);

    let clone =
        find_game_by_name_and_bios_and_system_id(&mut connection, "Test Clone", false, system.id)
            .await
            .unwrap();
    let clone_rom = find_rom_by_name_and_game_id(&mut connection, "Test Clone.rom", clone.id)
        .await
        .unwrap();
    let clone_only_rom =
        find_rom_by_name_and_game_id(&mut connection, "Test Clone Only.rom", clone.id)
            .await
            .unwrap();
    assert!(clone_rom.romfile_id.is_some());
    let clone_rom_path = rom_directory
        .path()
        .join(find_romfile_by_id(&mut connection, clone_rom.romfile_id.unwrap()).await.path);
    let clone_only_path = rom_directory
        .path()
        .join(find_romfile_by_id(&mut connection, clone_only_rom.romfile_id.unwrap()).await.path);
    assert!(clone_rom_path.is_file());
    assert!(clone_only_path.is_file());

    // when trimming back to SPLIT
    let matches = subcommand().get_matches_from(["rebuild-roms", "--all", "-m", "SPLIT"]);
    main(&mut connection, &matches, &progress_bar).await.unwrap();

    // then the clone's shared roms lost their files, the exclusive one is kept
    let system = find_system_by_id(&mut connection, system.id).await;
    assert_eq!(system.merging, Merging::Split as i64);

    let clone_rom = find_rom_by_name_and_game_id(&mut connection, "Test Clone.rom", clone.id)
        .await
        .unwrap();
    let clone_common_rom =
        find_rom_by_name_and_game_id(&mut connection, "Test Clone Common.rom", clone.id)
            .await
            .unwrap();
    let clone_only_rom =
        find_rom_by_name_and_game_id(&mut connection, "Test Clone Only.rom", clone.id)
            .await
            .unwrap();
    assert_eq!(clone_rom.romfile_id, None);
    assert_eq!(clone_common_rom.romfile_id, None);
    assert!(clone_only_rom.romfile_id.is_some());
    assert!(!clone_rom_path.is_file());
    assert!(clone_only_path.is_file());

    // the parent's files are untouched
    let parent =
        find_game_by_name_and_bios_and_system_id(&mut connection, "Test Parent", false, system.id)
            .await
            .unwrap();
    let parent_rom = find_rom_by_name_and_game_id(&mut connection, "Test Parent.rom", parent.id)
        .await
        .unwrap();
    let parent_romfile = find_romfile_by_id(&mut connection, parent_rom.romfile_id.unwrap()).await;
    assert!(
        rom_directory
            .path()
            .join(&parent_romfile.path)
            .starts_with(system_directory.join("Test Parent"))
    );
}

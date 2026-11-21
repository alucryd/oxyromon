use super::super::import_dats;
use super::super::import_roms;
use super::*;
use tempfile::TempDir;
use tokio::fs;

// The issue #331 Theme Park case: a release whose name tells of a preferred
// language wins over a release in a higher-ranked region whose name tells of
// no language at all
#[tokio::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = crate::config::TestDatabase::new();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    let tmp_directory =
        set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand()
        .get_matches_from(["import-dats", "tests/Language Test System (20200721).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let romfile_names = vec![
        "Language Game (USA).rom",
        "Language Game (Europe) (En,Fr).rom",
    ];
    for romfile_name in &romfile_names {
        let romfile_path = tmp_directory.join(romfile_name);
        fs::copy(test_directory.join(romfile_name), &romfile_path)
            .await
            .unwrap();
        let matches = import_roms::subcommand()
            .get_matches_from(["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
        import_roms::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();
    }

    let system = find_systems(&mut connection).await.remove(0);

    let all_regions = vec![];
    let one_regions = vec![Region::UnitedStates, Region::Europe];

    // when
    sort_system(
        &mut connection,
        &progress_bar,
        true,
        false,
        &system,
        &all_regions,
        &one_regions,
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        true,
        &PreferredRegion::None,
        &PreferredVersion::None,
        &[],
        &None,
        &None,
        false,
        true,
        true,
        &["Fr"],
    )
    .await
    .unwrap();

    // then
    let romfiles = find_romfiles_by_system_id(&mut connection, system.id).await;
    assert_eq!(2, romfiles.len());
    for romfile in &romfiles {
        let name = romfile.path.rsplit('/').next().unwrap();
        if name == "Language Game (Europe) (En,Fr).rom" {
            assert!(romfile.path.contains("1G1R/"), "{}", romfile.path);
        } else {
            assert_eq!("Language Game (USA).rom", name);
            assert!(romfile.path.contains("Trash/"), "{}", romfile.path);
        }
        assert!(rom_directory.path().join(&romfile.path).is_file());
    }
}

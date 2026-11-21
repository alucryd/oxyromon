use super::super::import_dats;
use super::super::import_roms;
use super::*;
use tempfile::TempDir;
use tokio::fs;

// Languages and regions tied, the existing version preference still
// elects: PREFER_VERSIONS new picks (Rev 1) over vanilla
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
        "Revision Game (USA) (En).rom",
        "Revision Game (USA) (Rev 1) (En).rom",
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
    let one_regions = vec![Region::UnitedStates];

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
        &PreferredVersion::New,
        &[],
        &None,
        &None,
        false,
        true,
        true,
        &["En"],
    )
    .await
    .unwrap();

    // then
    let romfiles = find_romfiles_by_system_id(&mut connection, system.id).await;
    assert_eq!(2, romfiles.len());
    let mut winners = vec!["Revision Game (USA) (Rev 1) (En).rom"];
    let mut losers = vec!["Revision Game (USA) (En).rom"];
    for romfile in &romfiles {
        let name = romfile.path.rsplit('/').next().unwrap();
        if let Some(i) = winners.iter().position(|w| *w == name) {
            assert!(romfile.path.contains("1G1R/"), "{}", romfile.path);
            winners.remove(i);
        } else {
            assert!(losers.contains(&name), "unexpected game {name}");
            assert!(romfile.path.contains("Trash/"), "{}", romfile.path);
            losers.retain(|l| *l != name);
        }
        assert!(rom_directory.path().join(&romfile.path).is_file());
    }
    assert!(winners.is_empty(), "missing winners {winners:?}");
    assert!(losers.is_empty(), "missing losers {losers:?}");
}

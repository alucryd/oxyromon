use super::super::database::*;
use super::super::import_dats;
use super::super::import_roms;
use super::super::util::*;
use super::*;
use tempfile::{NamedTempFile, TempDir};
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(&test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(&test_directory).unwrap();
    let tmp_directory =
        set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand().get_matches_from(&[
        "import-dats",
        "tests/Test System (20230112) (Parent-Clone) (Revisions).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let romfile_names = vec![
        "Test Game (USA, Europe).rom",
        "Test Game (Japan).rom",
        "Test Game (USA) (Rev 1).rom",
        "Test Game (USA, Europe) (Beta).rom",
    ];
    for romfile_name in &romfile_names {
        let romfile_path = tmp_directory.join(romfile_name);
        fs::copy(test_directory.join(romfile_name), &romfile_path)
            .await
            .unwrap();
        let matches = import_roms::subcommand()
            .get_matches_from(&["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
        import_roms::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();
    }

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();

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
        true,
        &PreferredRegion::None,
        &PreferredVersion::None,
        &[],
        &SubfolderScheme::None,
        &SubfolderScheme::None,
        false,
    )
    .await
    .unwrap();

    // then
    let romfiles = find_romfiles_by_system_id(&mut connection, system.id).await;
    assert_eq!(4, romfiles.len());

    let one_regions_indices = vec![0];
    let trash_indices = vec![1, 2, 3];

    for i in one_regions_indices {
        let romfile = romfiles.get(i).unwrap();
        assert_eq!(
            &system_directory
                .join("1G1R")
                .join(&romfile_names.get(i).unwrap())
                .strip_prefix(&rom_directory)
                .unwrap()
                .as_os_str()
                .to_str()
                .unwrap(),
            &romfile.path
        );
        assert!(rom_directory.path().join(&romfile.path).is_file());
    }

    for i in trash_indices {
        let romfile = romfiles.get(i).unwrap();
        assert_eq!(
            &system_directory
                .join("Trash")
                .join(&romfile_names.get(i).unwrap())
                .strip_prefix(&rom_directory)
                .unwrap()
                .as_os_str()
                .to_str()
                .unwrap(),
            &romfile.path
        );
        assert!(rom_directory.path().join(&romfile.path).is_file());
    }
}

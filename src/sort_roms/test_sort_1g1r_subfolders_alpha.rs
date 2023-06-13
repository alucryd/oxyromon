use super::super::database::*;
use super::super::import_dats;
use super::super::import_roms;
use super::super::util::*;
use super::*;
use async_std::fs;
use tempfile::{NamedTempFile, TempDir};

#[async_std::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(&test_directory).unwrap();
    set_rom_directory(PathBuf::from(rom_directory.path()));
    let tmp_directory = TempDir::new_in(&test_directory).unwrap();
    let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));

    let matches = import_dats::subcommand().get_matches_from(&[
        "import-dats",
        "tests/Test System (20200721) (Parent-Clone).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let romfile_names = vec![
        "42 Test Game (USA, Europe).rom",
        "Another Test Game (USA, Europe).rom",
        "Test Game (USA, Europe).rom",
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
    let system_directory = get_system_directory(&mut connection, &progress_bar, &system)
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
        true,
        &PreferredRegion::None,
        &PreferredVersion::None,
        &[],
        &SubfolderScheme::None,
        &SubfolderScheme::Alpha,
        false,
    )
    .await
    .unwrap();

    // then
    let romfiles = find_romfiles_by_system_id(&mut connection, system.id).await;
    assert_eq!(3, romfiles.len());

    for (i, romfile) in romfiles.iter().enumerate() {
        let first_char = &romfile_names.get(i).unwrap().chars().next().unwrap();
        assert_eq!(
            &system_directory
                .join("1G1R")
                .join(if first_char.is_ascii_alphabetic() {
                    first_char.to_ascii_uppercase().to_string()
                } else {
                    String::from("#")
                })
                .join(&romfile_names.get(i).unwrap())
                .as_os_str()
                .to_str()
                .unwrap(),
            &romfile.path
        );
        assert!(Path::new(&romfile.path).is_file().await);
    }
}

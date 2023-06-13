use super::super::config::{PreferredRegion, PreferredVersion};
use super::super::database::*;
use super::super::generate_playlists;
use super::super::import_dats;
use super::super::import_roms;
use super::*;
use async_std::fs;
use async_std::path::Path;
use std::env;
use tempfile::{NamedTempFile, TempDir};

#[async_std::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    env::set_var(
        "PATH",
        format!(
            "{}:{}",
            test_directory.as_os_str().to_str().unwrap(),
            env::var("PATH").unwrap()
        ),
    );
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
        "tests/Test System (20230105) (Multiple Discs).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &progress_bar, &system)
        .await
        .unwrap();

    let mut romfile_names: Vec<String> = Vec::new();
    for i in 1..=2 {
        romfile_names.push(format!("Test Game (USA, Europe) (Disc {}).iso", i));
    }

    for romfile_name in &romfile_names {
        let romfile_path = tmp_directory.join(&romfile_name);
        fs::copy(test_directory.join(&romfile_name), &romfile_path)
            .await
            .unwrap();
        let matches = import_roms::subcommand()
            .get_matches_from(&["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
        import_roms::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();
    }

    let matches = generate_playlists::subcommand().get_matches_from(&["generate-playlists", "-a"]);
    generate_playlists::main(&mut connection, &matches, &progress_bar)
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
        &SubfolderScheme::None,
        false,
    )
    .await
    .unwrap();

    // then
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 3);

    let roms_indices = vec![0, 1];

    for i in roms_indices {
        let romfile = romfiles.get(i).unwrap();
        assert_eq!(
            &system_directory
                .join("1G1R")
                .join(romfile_names.get(i).unwrap())
                .as_os_str()
                .to_str()
                .unwrap(),
            &romfile.path
        );
        assert!(Path::new(&romfile.path).is_file().await);
    }

    let romfile = romfiles.get(2).unwrap();
    assert_eq!(
        &system_directory
            .join("1G1R")
            .join("Test Game (USA, Europe).m3u")
            .as_os_str()
            .to_str()
            .unwrap(),
        &romfile.path
    );
    assert!(Path::new(&romfile.path).is_file().await);
}

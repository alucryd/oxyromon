use super::*;
use crate::import_dats;
use crate::import_roms;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

#[tokio::test]
async fn test() {
    // given
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
        .get_matches_from(["import-dats", "tests/Test System (20261114) (Game Boy).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let romfile_path = tmp_directory.join("Test Game (USA, Europe).gb.7z");
    tokio::fs::copy(
        test_directory.join("Test Game (USA, Europe).gb.7z"),
        &romfile_path,
    )
    .await
    .unwrap();
    let matches = import_roms::subcommand()
        .get_matches_from(["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();
    let ore_path = system_directory.join("Test Game (USA, Europe).ore");
    let store = system_directory.join(ore_rs::STORE_DIR);
    let rom = std::fs::read(test_directory.join("Test Game (USA, Europe).gb")).unwrap();
    let thin = |path: &Path| {
        ore_rs::Reader::inspect(path)
            .unwrap()
            .store_uuid()
            .is_some()
    };
    let reads_back = |path: &Path| {
        let mut reader = ore_rs::Reader::open(path).unwrap();
        let mut bytes = vec![0; rom.len()];
        reader.read_at(0, &mut bytes).unwrap();
        bytes == rom
    };

    // when the store is on, the run one batch: written straight into it
    set_bool(&mut connection, "ORE_STORE", true, Some(system.id)).await;
    let matches = subcommand().get_matches_from(["convert-roms", "-f", "ORE", "-a", "-c"]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then the ORE is thin, the database points to it, and the archive is gone
    assert!(thin(&ore_path));
    assert!(reads_back(&ore_path));
    assert!(
        !system_directory
            .join("Test Game (USA, Europe).gb.7z")
            .exists()
    );
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);
    assert!(romfiles[0].path.ends_with("Test Game (USA, Europe).ore"));
    // and nothing is left of the batch
    assert!(
        std::fs::read_dir(&store)
            .unwrap()
            .all(|entry| { !entry.unwrap().path().to_string_lossy().ends_with(".part") })
    );
}

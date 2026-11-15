use super::super::convert_roms;
use super::super::database::*;
use super::super::import_dats;
use super::super::import_roms;
use super::super::util::*;
use super::*;
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};

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

    // a store written with parity
    set_bool(&mut connection, "ORE_STORE", true, Some(system.id)).await;
    let matches = convert_roms::subcommand().get_matches_from(["convert-roms", "-f", "ORE", "-a"]);
    convert_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();
    assert!(thin(&ore_path));
    let size = |dir: &Path| -> u64 {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().metadata().unwrap().len())
            .sum()
    };
    let with_parity = size(&store);

    // when its parity is turned off and the store compacted
    set_integer(&mut connection, "ORE_STORE_PARITY", 0, Some(system.id)).await;
    let matches = subcommand().get_matches_from(["compact-roms", "-a"]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then it is sealed again without it, and the game reads on
    assert!(size(&store) < with_parity);
    assert!(reads_back(&ore_path));

    // and recompressed, it reads on still
    let matches = subcommand().get_matches_from(["compact-roms", "-a", "-r"]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();
    assert!(reads_back(&ore_path));
    assert_eq!(find_romfiles(&mut connection).await.len(), 1);
}

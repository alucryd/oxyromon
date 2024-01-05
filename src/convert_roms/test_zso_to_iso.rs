use super::super::database::*;
use super::super::import_dats;
use super::super::import_roms;
use super::*;
use std::env;
use tempfile::{NamedTempFile, TempDir};
use tokio::fs;

#[tokio::test]
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

    let matches = import_dats::subcommand()
        .get_matches_from(&["import-dats", "tests/Test System (20200721).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let romfile_path = tmp_directory.join("Test Game (USA, Europe).zso");
    fs::copy(
        test_directory.join("Test Game (USA, Europe).zso"),
        &romfile_path,
    )
    .await
    .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();

    let matches = import_roms::subcommand()
        .get_matches_from(&["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap()).await;
    let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
    roms_by_game_id.insert(roms[0].game_id, roms);
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    romfiles_by_id.insert(romfile.id, romfile);

    // when
    to_original(
        &mut connection,
        &progress_bar,
        &system,
        roms_by_game_id,
        romfiles_by_id,
    )
    .await
    .unwrap();

    // then
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), 1);
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);

    let rom = roms.get(0).unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe).iso");

    let romfile = romfiles.get(0).unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe).iso")
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(Path::new(&romfile.path).is_file());
    assert_eq!(rom.romfile_id, Some(romfile.id));
}

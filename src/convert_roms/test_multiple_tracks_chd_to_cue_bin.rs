use super::super::database::*;
use super::super::import_dats;
use super::super::import_roms;
use super::*;
use std::path::{Path, PathBuf};
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
    set_rom_directory(PathBuf::from(rom_directory.path()));
    let tmp_directory = TempDir::new_in(&test_directory).unwrap();
    let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));

    let matches = import_dats::subcommand()
        .get_matches_from(&["import-dats", "tests/Test System (20200721).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Multiple Tracks).cue");
    fs::copy(
        test_directory.join("Test Game (USA, Europe) (Multiple Tracks).cue"),
        &romfile_path,
    )
    .await
    .unwrap();
    let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Multiple Tracks).chd");
    fs::copy(
        test_directory.join("Test Game (USA, Europe) (Multiple Tracks).chd"),
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
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    for rom in &roms {
        let romfile = find_romfile_by_id(&mut connection, rom.romfile_id.unwrap()).await;
        romfiles_by_id.insert(romfile.id, romfile);
    }
    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    roms_by_game_id.insert(roms[0].game_id, roms);

    // when
    to_original(
        &mut connection,
        &progress_bar,
        &system,
        roms_by_game_id,
        romfiles_by_id,
        true,
        &HashAlgorithm::Crc,
    )
    .await
    .unwrap();

    // then
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), 3);
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 3);

    let rom = roms.first().unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe) (Track 01).bin");

    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe) (Track 01).bin")
            .strip_prefix(&rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(rom_directory.path().join(&romfile.path).is_file());
    assert_eq!(rom.romfile_id, Some(romfile.id));

    let rom = roms.get(1).unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe) (Track 02).bin");

    let romfile = romfiles.get(1).unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe) (Track 02).bin")
            .strip_prefix(&rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(rom_directory.path().join(&romfile.path).is_file());
    assert_eq!(rom.romfile_id, Some(romfile.id));

    let rom = roms.get(2).unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe).cue");

    let romfile = romfiles.get(2).unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe).cue")
            .strip_prefix(&rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(rom_directory.path().join(&romfile.path).is_file());
    assert_eq!(rom.romfile_id, Some(romfile.id));
}

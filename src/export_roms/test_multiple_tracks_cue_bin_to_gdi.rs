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

    let rom_directory = TempDir::new_in(test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    let tmp_directory =
        set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand()
        .get_matches_from(["import-dats", "tests/Test System (20200721).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let mut romfile_paths: Vec<PathBuf> = vec![];
    let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Multiple Tracks).cue");
    fs::copy(
        test_directory.join("Test Game (USA, Europe) (Multiple Tracks).cue"),
        &romfile_path,
    )
    .await
    .unwrap();
    romfile_paths.push(romfile_path);
    let romfile_path = tmp_directory.join("Test Game (USA, Europe) (CUE BIN) (Track 01).bin");
    fs::copy(
        test_directory.join("Test Game (USA, Europe) (CUE BIN) (Track 01).bin"),
        &romfile_path,
    )
    .await
    .unwrap();
    romfile_paths.push(romfile_path);
    let romfile_path = tmp_directory.join("Test Game (USA, Europe) (CUE BIN) (Track 02).bin");
    fs::copy(
        test_directory.join("Test Game (USA, Europe) (CUE BIN) (Track 02).bin"),
        &romfile_path,
    )
    .await
    .unwrap();
    romfile_paths.push(romfile_path);

    let system = find_systems(&mut connection).await.remove(0);

    for romfile_path in romfile_paths {
        let matches = import_roms::subcommand()
            .get_matches_from(["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
        import_roms::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();
    }

    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    for rom in &roms {
        let romfile = find_romfile_by_id(&mut connection, rom.romfile_id.unwrap()).await;
        romfiles_by_id.insert(romfile.id, romfile);
    }
    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    roms_by_game_id.insert(roms[0].game_id, roms);

    let destination_directory = tmp_directory.join("destination");
    create_directory(&progress_bar, &destination_directory, true)
        .await
        .unwrap();

    // when
    to_gdi(
        &mut connection,
        &progress_bar,
        &destination_directory,
        roms_by_game_id,
        romfiles_by_id,
    )
    .await
    .unwrap();

    // then
    // What gdidrop writes for the same set, but for its " [gdidrop]" suffix:
    // track 1 past its 231-sector pregap, track 2 whole.
    assert_eq!(
        fs::read_to_string(destination_directory.join("Test Game (USA, Europe) (CUE BIN).gdi"))
            .await
            .unwrap(),
        "2\n\
         1 231 4 2352 \"Test Game (USA, Europe) (CUE BIN) (Track 01).bin\" 0\n\
         2 8635 0 2352 \"Test Game (USA, Europe) (CUE BIN) (Track 02).raw\" 0\n"
    );
    let track_01 = fs::metadata(
        destination_directory.join("Test Game (USA, Europe) (CUE BIN) (Track 01).bin"),
    )
    .await
    .unwrap();
    assert_eq!(track_01.len(), (8635 - 231) * 2352);
}

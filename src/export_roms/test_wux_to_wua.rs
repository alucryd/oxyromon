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
        .get_matches_from(["import-dats", "tests/Test System (20261025) (WUD).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let romfile_path = tmp_directory.join("Test Game (USA).wux");
    fs::copy(test_directory.join("Test Game (USA).wux"), &romfile_path)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);

    let matches = import_roms::subcommand()
        .get_matches_from(["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap()).await;
    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    roms_by_game_id.insert(roms[0].game_id, roms);
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    romfiles_by_id.insert(romfile.id, romfile);

    let destination_directory = tmp_directory.join("destination");
    create_directory(&progress_bar, &destination_directory, true)
        .await
        .unwrap();

    // when
    to_wua(
        &mut connection,
        &progress_bar,
        &destination_directory,
        roms_by_game_id,
        romfiles_by_id,
    )
    .await
    .unwrap();

    // then
    // a ZArchive, ending with its version and magic
    let wua = fs::read(destination_directory.join("Test Game (USA).wua"))
        .await
        .unwrap();
    assert_eq!(
        wua[wua.len() - 8..],
        [0x61, 0xbf, 0x3a, 0x01, 0x16, 0x9f, 0x52, 0xd6]
    );
    // lossless, its update partition in the store the destination's WUAs share
    let wua = destination_directory.join("Test Game (USA).wua");
    assert!(wux_rs::disc_info(&wua).unwrap().is_some());
    for entry in wux_rs::store_entries(&wua).unwrap() {
        assert!(
            destination_directory
                .join(wux_rs::STORE_DIR)
                .join(entry)
                .is_file()
        );
    }
}

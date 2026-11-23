use crate::convert_roms;
use crate::import_dats;
use crate::import_roms;
use super::*;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tokio::fs;

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
    fs::copy(
        test_directory.join("Test Game (USA, Europe).gb.7z"),
        &romfile_path,
    )
    .await
    .unwrap();

    let system = find_systems(&mut connection).await.remove(0);

    let matches = import_roms::subcommand()
        .get_matches_from(["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // a thin ORE, its chunks in the system's store
    set_bool(&mut connection, "ORE_STORE", true, Some(system.id)).await;
    let matches = convert_roms::subcommand().get_matches_from(["convert-roms", "-f", "ORE", "-a"]);
    convert_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let load = async |connection: &mut SqliteConnection| {
        let roms = find_roms_with_romfile_by_system_id(connection, system.id).await;
        let romfile = find_romfile_by_id(connection, roms[0].romfile_id.unwrap()).await;
        assert!(romfile.path.ends_with(".ore"));
        let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
        roms_by_game_id.insert(roms[0].game_id, roms);
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        romfiles_by_id.insert(romfile.id, romfile);
        (roms_by_game_id, romfiles_by_id)
    };
    let rom = fs::read(test_directory.join("Test Game (USA, Europe).gb"))
        .await
        .unwrap();

    // when exported as its original
    let original_directory = tmp_directory.join("original");
    create_directory(&progress_bar, &original_directory, true)
        .await
        .unwrap();
    let (roms_by_game_id, romfiles_by_id) = load(&mut connection).await;
    let games_by_id: HashMap<i64, Game> = find_games_by_system_id(&mut connection, system.id)
        .await
        .into_iter()
        .map(|game| (game.id, game))
        .collect();
    to_original(
        &mut connection,
        &progress_bar,
        &original_directory,
        &system,
        games_by_id,
        roms_by_game_id,
        romfiles_by_id,
    )
    .await
    .unwrap();

    // then the ROM comes out under its name, read through the store
    let exported = fs::read(original_directory.join("Test Game (USA, Europe).gb"))
        .await
        .unwrap();
    assert!(exported == rom);

    // when exported as an ORE
    let ore_directory = tmp_directory.join("ore");
    create_directory(&progress_bar, &ore_directory, true)
        .await
        .unwrap();
    let (roms_by_game_id, romfiles_by_id) = load(&mut connection).await;
    to_ore(
        &mut connection,
        &progress_bar,
        &ore_directory,
        roms_by_game_id,
        romfiles_by_id,
    )
    .await
    .unwrap();

    // then it is standalone, reading with no store beside it
    let ore = ore_directory.join("Test Game (USA, Europe).ore");
    let mut reader = ore_rs::Reader::open(&ore).unwrap();
    assert!(reader.store_uuid().is_none());
    let mut bytes = vec![0; rom.len()];
    reader.read_at(0, &mut bytes).unwrap();
    assert!(bytes == rom);
}

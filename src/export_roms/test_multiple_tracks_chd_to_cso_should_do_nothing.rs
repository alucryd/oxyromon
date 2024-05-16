use super::super::database::*;
use super::super::import_dats;
use super::super::import_roms;
use super::*;
use std::env;
use std::path::{Path, PathBuf};
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

    let matches = import_roms::subcommand()
        .get_matches_from(&["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    for rom in &roms {
        let romfile = find_romfile_by_id(&mut connection, rom.romfile_id.unwrap()).await;
        romfiles_by_id.insert(romfile.id, romfile);
    }
    roms_by_game_id.insert(roms[0].game_id, roms);

    let system_directory = tmp_directory.join("destination");
    create_directory(&progress_bar, &system_directory, true)
        .await
        .unwrap();

    // when
    to_cso(
        &mut connection,
        &progress_bar,
        &system_directory,
        roms_by_game_id,
        romfiles_by_id,
    )
    .await
    .unwrap();

    // then
    assert!(!system_directory
        .join("Test Game (USA, Europe).cso")
        .is_file());
}

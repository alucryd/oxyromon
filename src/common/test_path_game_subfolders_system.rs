use indicatif::ProgressBar;
use tempfile::{NamedTempFile, TempDir};

use super::super::import_dats;
use super::*;

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

    let matches = import_dats::subcommand()
        .get_matches_from(["import-dats", "tests/Test System (20200721).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();
    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();

    let game = Game {
        id: 1,
        name: String::from("game name"),
        description: String::from(""),
        comment: None,
        external_id: None,
        device: false,
        bios: false,
        jbfolder: false,
        regions: String::from(""),
        sorting: Sorting::AllRegions as i64,
        completion: 0,
        system_id: system.id,
        parent_id: None,
        bios_id: None,
        playlist_id: None,
        categories: None,
    };
    let rom = Rom {
        id: 1,
        name: String::from("rom name.rom"),
        bios: false,
        disk: false,
        size: 1,
        crc: Some(String::from("")),
        md5: Some(String::from("")),
        sha1: Some(String::from("")),
        rom_status: None,
        game_id: 1,
        romfile_id: Some(1),
        parent_id: None,
        original: true,
    };
    let romfile = Romfile {
        id: 1,
        path: String::from("romfile.rom"),
        size: 0,
        parent_id: None,
        romfile_type: RomfileType::Romfile as i64,
    };

    // when: off globally, on for this system
    set_string(&mut connection, "GAME_SUBFOLDERS", "never", None).await;
    set_string(
        &mut connection,
        "GAME_SUBFOLDERS",
        "always",
        Some(system.id),
    )
    .await;
    let nested = romfile
        .as_common(&mut connection)
        .await
        .unwrap()
        .get_sorted_path(&mut connection, &system, &game, &rom, &None, &None)
        .await
        .unwrap();

    // and: on globally, off for this system
    set_string(&mut connection, "GAME_SUBFOLDERS", "always", None).await;
    set_string(&mut connection, "GAME_SUBFOLDERS", "never", Some(system.id)).await;
    let flat = romfile
        .as_common(&mut connection)
        .await
        .unwrap()
        .get_sorted_path(&mut connection, &system, &game, &rom, &None, &None)
        .await
        .unwrap();

    // then: the system's own value wins both ways
    assert_eq!(
        nested,
        system_directory.join("game name").join("rom name.rom")
    );
    assert_eq!(flat, system_directory.join("rom name.rom"));
}

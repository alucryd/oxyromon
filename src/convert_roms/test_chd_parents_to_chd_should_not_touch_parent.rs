use super::super::database::*;
use super::super::generate_playlists;
use super::super::import_dats;
use super::super::import_roms;
use super::*;
use std::path::{Path, PathBuf};
use std::{env, time::SystemTime};
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

    let matches = import_dats::subcommand().get_matches_from(&[
        "import-dats",
        "tests/Test System (20230105) (Multiple Discs).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();

    for i in 1..=2 {
        let romfile_name = format!("Test Game (USA, Europe) (Disc {}).iso", i);
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

    let games = find_complete_games_by_system_id(&mut connection, system.id).await;
    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    for game in &games {
        let roms = find_roms_with_romfile_by_game_ids(&mut connection, &[game.id]).await;
        for rom in &roms {
            let romfile = find_romfile_by_id(&mut connection, rom.romfile_id.unwrap()).await;
            romfiles_by_id.insert(romfile.id, romfile);
        }
        roms_by_game_id.insert(game.id, roms);
    }
    let games_by_id: HashMap<i64, Game> = games.into_iter().map(|game| (game.id, game)).collect();

    to_chd(
        &mut connection,
        &progress_bar,
        games_by_id,
        roms_by_game_id,
        romfiles_by_id,
        false,
        true,
        true,
        &HashAlgorithm::Crc,
        &[],
        &None,
        &[],
        &None,
        &[],
        &None,
        &[],
        &None,
        true,
        false,
    )
    .await
    .unwrap();

    let games = find_complete_games_by_system_id(&mut connection, system.id).await;
    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    let mut romfiles_mtimes: Vec<SystemTime> = Vec::new();
    for game in &games {
        let roms = find_roms_with_romfile_by_game_ids(&mut connection, &[game.id]).await;
        for rom in &roms {
            let romfile = find_romfile_by_id(&mut connection, rom.romfile_id.unwrap()).await;
            romfiles_mtimes.push(
                romfile
                    .as_common(&mut connection)
                    .await
                    .unwrap()
                    .path
                    .metadata()
                    .unwrap()
                    .modified()
                    .unwrap(),
            );
            romfiles_by_id.insert(romfile.id, romfile);
        }
        roms_by_game_id.insert(game.id, roms);
    }
    let games_by_id: HashMap<i64, Game> = games.into_iter().map(|game| (game.id, game)).collect();

    // when
    to_chd(
        &mut connection,
        &progress_bar,
        games_by_id,
        roms_by_game_id,
        romfiles_by_id,
        true,
        true,
        true,
        &HashAlgorithm::Crc,
        &[],
        &None,
        &[],
        &None,
        &[],
        &None,
        &[],
        &None,
        true,
        false,
    )
    .await
    .unwrap();

    // then
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), 2);
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 3);

    let rom = roms.get(0).unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe) (Disc 1).iso");

    let romfile = romfiles.get(0).unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe) (Disc 1).chd")
            .strip_prefix(&rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(rom_directory.path().join(&romfile.path).is_file());
    assert_eq!(rom.romfile_id, Some(romfile.id));
    assert!(romfile.parent_id.is_none());
    assert_eq!(
        romfile
            .as_common(&mut connection)
            .await
            .unwrap()
            .path
            .metadata()
            .unwrap()
            .modified()
            .unwrap()
            .cmp(romfiles_mtimes.get(0).unwrap()),
        Ordering::Equal
    );

    let rom = roms.get(1).unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe) (Disc 2).iso");

    let romfile = romfiles.get(1).unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe) (Disc 2).chd")
            .strip_prefix(&rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(rom_directory.path().join(&romfile.path).is_file());
    assert_eq!(rom.romfile_id, Some(romfile.id));
    assert!(romfile.parent_id.is_some());
    assert_eq!(
        romfile
            .as_common(&mut connection)
            .await
            .unwrap()
            .path
            .metadata()
            .unwrap()
            .modified()
            .unwrap()
            .cmp(romfiles_mtimes.get(1).unwrap()),
        Ordering::Greater
    );
}

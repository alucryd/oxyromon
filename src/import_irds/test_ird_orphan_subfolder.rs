use super::super::common::{AsCommon, CommonRomfile, FromPath, Persist};
use super::super::config::*;
use super::super::import_dats;
use super::*;
use md5::{Digest, Md5};
use std::path::PathBuf;
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
    set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand()
        .get_matches_from(["import-dats", "tests/Test System (20240704) (IRD).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let ird_path = test_directory.join("Test Game (USA).ird");
    let game = find_games(&mut connection).await.remove(0);
    let (ird_file, header) = parse_ird(&ird_path).await.unwrap();
    let parent_roms = find_roms_by_game_id_no_parents(&mut connection, game.id).await;
    import_ird(
        &mut connection,
        &progress_bar,
        &game,
        &ird_file,
        header,
        parent_roms.first(),
    )
    .await
    .unwrap();

    let game = find_games(&mut connection).await.remove(0);
    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();
    let game_directory = system_directory.join(&game.name);

    // a file deep in the JB folder, as an older IRD described it, and one at its root
    let roms = find_roms(&mut connection).await;
    let deep_rom = roms
        .iter()
        .filter(|rom| rom.game_id == game.id)
        .max_by_key(|rom| rom.name.matches('/').count())
        .unwrap();
    let root_rom = roms
        .iter()
        .find(|rom| rom.game_id == game.id && !rom.name.contains('/'))
        .unwrap();
    let old_content = b"content an older IRD described";
    let deep_path = game_directory.join(&deep_rom.name);
    let root_path = game_directory.join(&root_rom.name);
    std::fs::create_dir_all(deep_path.parent().unwrap()).unwrap();
    std::fs::write(&deep_path, old_content).unwrap();
    std::fs::write(&root_path, b"root").unwrap();
    let romfile_id = CommonRomfile::from_path(&deep_path)
        .unwrap()
        .create(&mut connection, &progress_bar, RomfileType::Romfile)
        .await
        .unwrap();
    update_rom_romfile(&mut connection, deep_rom.id, Some(romfile_id)).await;
    let old_md5 = Md5::digest(old_content)
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect::<String>();
    update_rom(
        &mut connection,
        deep_rom.id,
        &deep_rom.name,
        old_content.len() as i64,
        &old_md5,
        game.id,
        deep_rom.parent_id,
        false,
    )
    .await;

    let (ird_file, header) = parse_ird(&ird_path).await.unwrap();
    let parent_roms = find_roms_by_game_id_no_parents(&mut connection, game.id).await;

    // when
    import_ird(
        &mut connection,
        &progress_bar,
        &game,
        &ird_file,
        header,
        parent_roms.first(),
    )
    .await
    .unwrap();

    // then
    // the file no longer matches, so it goes to the trash, and every directory it
    // left empty goes with it, up to the first one that still holds something
    let romfiles = find_romfiles(&mut connection).await;
    let romfile = romfiles
        .iter()
        .find(|romfile| romfile.id == romfile_id)
        .unwrap();
    let romfile_path = romfile.as_common(&mut connection).await.unwrap().path;
    assert_eq!(
        romfile_path,
        system_directory
            .join("Trash")
            .join(deep_path.file_name().unwrap())
    );
    assert!(romfile_path.is_file());
    assert!(
        !game_directory
            .join(deep_rom.name.split('/').next().unwrap())
            .exists()
    );
    assert!(game_directory.is_dir());
    assert!(root_path.is_file());
}

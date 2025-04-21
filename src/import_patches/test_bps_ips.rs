use super::super::config::*;
use super::super::database::*;
use super::super::import_dats;
use super::super::import_roms;
use super::*;
use std::path::PathBuf;
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
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(&test_directory).unwrap();
    let tmp_directory =
        set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand()
        .get_matches_from(&["import-dats", "tests/Test System (20240908) (Patches).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let romfile_path = tmp_directory.join("Test Game (USA, Europe).rom");
    fs::copy(
        test_directory.join("Test Game (USA, Europe).rom"),
        &romfile_path,
    )
    .await
    .unwrap();

    let bps_path = tmp_directory.join("Test Game (USA, Europe) (BPS).bps");
    fs::copy(
        test_directory.join("Test Game (USA, Europe).bps"),
        &bps_path,
    )
    .await
    .unwrap();
    let ips_path = tmp_directory.join("Test Game (USA, Europe) (IPS).ips");
    fs::copy(
        test_directory.join("Test Game (USA, Europe).ips"),
        &ips_path,
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

    // when
    import_patch(
        &mut connection,
        &progress_bar,
        &bps_path,
        &PatchType::Bps,
        false,
        false,
    )
    .await
    .unwrap();
    import_patch(
        &mut connection,
        &progress_bar,
        &ips_path,
        &PatchType::Ips,
        false,
        false,
    )
    .await
    .unwrap();

    // then
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), 1);
    let rom = roms.first().unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe).rom");
    let patches = find_patches_by_rom_id(&mut connection, rom.id).await;
    assert_eq!(patches.len(), 2);
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 3);

    let patch = patches.get(0).unwrap();
    assert_eq!(patch.name, "Test Game (USA, Europe) (BPS)");
    assert_eq!(patch.index, 0);
    assert_eq!(patch.rom_id, rom.id);
    let romfile = romfiles.get(0).unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe).bps")
            .strip_prefix(&rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(rom_directory.path().join(&romfile.path).is_file());
    assert_eq!(patch.romfile_id, romfile.id);

    let patch = patches.get(1).unwrap();
    assert_eq!(patch.name, "Test Game (USA, Europe) (IPS)");
    assert_eq!(patch.index, 1);
    assert_eq!(patch.rom_id, rom.id);

    let romfile = romfiles.get(1).unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe).ips1")
            .strip_prefix(&rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(rom_directory.path().join(&romfile.path).is_file());
    assert_eq!(patch.romfile_id, romfile.id);
}

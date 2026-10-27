use super::super::config::*;
use super::super::import_dats;
use super::super::import_roms;
use super::*;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

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

    let matches = import_dats::subcommand().get_matches_from([
        "import-dats",
        "tests/Test System (20200721) (MAME Merged).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // a ZIP holding files of two games is imported as loose files, one
    // directory per game
    let zip_path = tmp_directory.join("Test Parent.zip");
    let mut writer = ZipWriter::new(File::create(&zip_path).unwrap());
    for (name, fixture) in [
        ("Test Program.rom", "Test Game (USA, Europe).rom"),
        ("Test Common.rom", "Test Game (Japan).rom"),
        ("Test Clone 1.rom", "Test Game (USA) (Rev 1).rom"),
    ] {
        writer
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(&std::fs::read(test_directory.join(fixture)).unwrap())
            .unwrap();
    }
    writer.finish().unwrap();
    let matches =
        import_roms::subcommand().get_matches_from(["import-roms", zip_path.to_str().unwrap()]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();
    assert!(system_directory.join("Test Parent").is_dir());
    assert!(system_directory.join("Test Clone 1").is_dir());

    // when
    purge_system(&mut connection, &progress_bar, &system)
        .await
        .unwrap();

    // then the game directories are gone, and the files are in the trash
    assert!(!system_directory.join("Test Parent").exists());
    assert!(!system_directory.join("Test Clone 1").exists());
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 3);
    for romfile in romfiles {
        assert!(romfile.path.contains("Trash"), "{}", romfile.path);
    }
}

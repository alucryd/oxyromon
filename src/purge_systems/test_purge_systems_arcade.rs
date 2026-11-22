use super::*;
use crate::test_support::TestEnv;
use std::fs::File;
use std::io::Write;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721) (MAME Merged).dat").await;

    // a ZIP holding files of two games is imported as loose files, one
    // directory per game
    let zip_path = env.tmp_directory.join("Test Parent.zip");
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
            .write_all(&std::fs::read(env.fixture_source(fixture)).unwrap())
            .unwrap();
    }
    writer.finish().unwrap();
    env.import_roms(&[&zip_path]).await;

    let system = env.system().await;
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();
    assert!(system_directory.join("Test Parent").is_dir());
    assert!(system_directory.join("Test Clone 1").is_dir());

    // when
    purge_system(&mut env.connection, &env.progress_bar, &system)
        .await
        .unwrap();

    // then the game directories are gone, and the files are in the trash
    assert!(!system_directory.join("Test Parent").exists());
    assert!(!system_directory.join("Test Clone 1").exists());
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 3);
    for romfile in romfiles {
        assert!(romfile.path.contains("Trash"), "{}", romfile.path);
    }
}

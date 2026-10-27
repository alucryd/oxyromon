// A merged ZIP is imported as loose files, one directory per game, and a ROM
// name may have no extension, as HBMAME's "yrw801-m" does.

use super::super::check_roms;
use super::super::import_dats;
use super::super::import_roms;
use super::*;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use tempfile::{NamedTempFile, TempDir};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

const DAT: &str = r#"<?xml version="1.0"?>
<datafile>
	<header>
		<name>Test System (MAME Extensionless)</name>
		<description>Test System (MAME Extensionless)</description>
		<version>20200721</version>
	</header>
	<machine name="Test Parent">
		<description>Test Parent</description>
		<rom name="Test Program.rom" size="256" crc="cc721e14" sha1="4b3e49f0f22c7ce5f1eb7c30a2bbd6fb4fdc6f3d" />
		<rom name="Test Common" size="256" crc="310212e8" sha1="5fcce2e41101d4e0f22a9279e65839145fa61846" />
	</machine>
	<machine name="Test Clone" cloneof="Test Parent" romof="Test Parent">
		<description>Test Clone</description>
		<rom name="Test Clones.rom" size="256" crc="e2565a1f" sha1="91a6396e9069879aa6e6cc18efc15429d219d4e0" />
		<rom name="Test Common" merge="Test Common" size="256" crc="310212e8" sha1="5fcce2e41101d4e0f22a9279e65839145fa61846" />
	</machine>
</datafile>
"#;

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

    let dat_path = tmp_directory.join("Test System.dat");
    std::fs::write(&dat_path, DAT).unwrap();
    let matches =
        import_dats::subcommand().get_matches_from(["import-dats", dat_path.to_str().unwrap()]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let zip_path = tmp_directory.join("Test Parent.zip");
    let mut writer = ZipWriter::new(File::create(&zip_path).unwrap());
    for (name, fixture) in [
        ("Test Program.rom", "Test Game (USA, Europe).rom"),
        ("Test Common", "Test Game (Japan).rom"),
        ("Test Clones.rom", "Test Game (USA, Europe) (Beta).rom"),
    ] {
        writer
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(&std::fs::read(test_directory.join(fixture)).unwrap())
            .unwrap();
    }
    writer.finish().unwrap();
    let matches = import_roms::subcommand().get_matches_from([
        "import-roms",
        "-u",
        "skip",
        zip_path.to_str().unwrap(),
    ]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_arcade_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();
    assert!(system_directory.join("Test Parent/Test Common").is_file());

    // when
    let matches = subcommand().get_matches_from(["rebuild-roms", "--all", "-m", "NON_MERGED"]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then the clone has its parent's extensionless file, loose like its own
    assert!(system_directory.join("Test Clone/Test Common").is_file());
    assert!(system_directory.join("Test Clone/Test Clones.rom").is_file());
    assert!(!system_directory.join("Test Clone.zip").exists());

    // and check-roms reads the extensionless files without trashing them
    let before = find_romfiles(&mut connection).await.len();
    let matches = check_roms::subcommand().get_matches_from(["check-roms", "-a"]);
    check_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();
    assert_eq!(before, find_romfiles(&mut connection).await.len());
}

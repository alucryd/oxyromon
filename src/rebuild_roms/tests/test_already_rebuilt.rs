// A set already non-merged, as a first rebuild leaves it, still rebuilds what is
// imported into it later: a clone's completion then counts its parent's files,
// which it does not hold yet, and that must not keep it from getting them.

use super::*;
use crate::test_support::TestEnv;
use std::fs::File;
use std::io::Write;
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

async fn rebuild(current: Merging, target: &str) {
    // given
    let mut env = TestEnv::new().await;
    let dat_path = env.tmp_directory.join("Test System.dat");
    std::fs::write(&dat_path, DAT).unwrap();
    env.run(&["import-dats", dat_path.to_str().unwrap()])
        .await
        .unwrap();

    let system = find_arcade_systems(&mut env.connection).await.remove(0);
    update_system_merging(&mut env.connection, system.id, current).await;

    let zip_path = env.tmp_directory.join("Test Parent.zip");
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
            .write_all(&std::fs::read(env.fixture_source(fixture)).unwrap())
            .unwrap();
    }
    writer.finish().unwrap();
    env.run(&["import-roms", "-u", "skip", zip_path.to_str().unwrap()])
        .await
        .unwrap();

    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();
    assert!(system_directory.join("Test Parent/Test Common").is_file());

    // when
    env.run(&["rebuild-roms", "--all", "-f", "-m", target])
        .await
        .unwrap();

    // then the clone has its parent's extensionless file, loose like its own
    assert!(system_directory.join("Test Clone/Test Common").is_file());
    assert!(
        system_directory
            .join("Test Clone/Test Clones.rom")
            .is_file()
    );
    assert!(!system_directory.join("Test Clone.zip").exists());

    // and it counts as complete
    let game = find_game_by_name_and_bios_and_system_id(
        &mut env.connection,
        "Test Clone",
        false,
        system.id,
    )
    .await
    .unwrap();
    assert_eq!(game.completion, Completion::Full as i64);
}

#[tokio::test]
async fn test() {
    rebuild(Merging::NonMerged, "NON_MERGED").await;
    rebuild(Merging::FullNonMerged, "FULL_NON_MERGED").await;
    rebuild(Merging::FullNonMerged, "NON_MERGED").await;
}

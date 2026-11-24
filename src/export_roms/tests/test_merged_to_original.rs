use super::*;
use crate::rebuild_roms::merged_set::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;

    import_split_set(&mut env).await;
    rebuild(&mut env.connection, &env.progress_bar, "MERGED").await;
    let destination_directory = env.tmp_directory.join("destination");
    std::fs::create_dir(&destination_directory).unwrap();

    // when
    let matches = subcommand().get_matches_from([
        "export-roms",
        "-f",
        "ORIGINAL",
        "-s",
        "Test System (MAME Merged)",
        "-d",
        destination_directory.to_str().unwrap(),
    ]);
    main(&mut env.connection, &matches, &env.progress_bar)
        .await
        .unwrap();

    // then each game gets its own directory, with the files under their own names
    for (game, rom, fixture) in [
        (
            "Test Parent",
            "Test Program.rom",
            "Test Game (USA, Europe).rom",
        ),
        ("Test Parent", "Test Common.rom", "Test Game (Japan).rom"),
        (
            "Test Clone 1",
            "Test Program.rom",
            "Test Game (USA) (Rev 1).rom",
        ),
        (
            "Test Clone 1",
            "Test Clones.rom",
            "Test Game (USA, Europe) (Beta).rom",
        ),
        (
            "Test Clone 2",
            "Test Clones.rom",
            "Test Game (USA, Europe) (Beta).rom",
        ),
    ] {
        let path = destination_directory.join(game).join(rom);
        assert_eq!(
            std::fs::read(&path).unwrap_or_else(|_| panic!("Missing {:?}", path)),
            std::fs::read(env.fixture_source(fixture)).unwrap()
        );
    }
    assert!(
        !destination_directory
            .join("Test Clone 1")
            .join("Test Clone 1")
            .exists()
    );
}

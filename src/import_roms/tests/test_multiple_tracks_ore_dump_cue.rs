use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    // an ORE of a cue sheet unlike the DAT's: the tracks are Redump's
    let dump_directory = env.tmp_directory.join("dump");
    fs::create_dir(&dump_directory).await.unwrap();
    for track in [
        "Test Game (USA, Europe) (CUE BIN) (Track 01).bin",
        "Test Game (USA, Europe) (CUE BIN) (Track 02).bin",
    ] {
        fs::copy(env.fixture_source(track), dump_directory.join(track))
            .await
            .unwrap();
    }
    let dat_cue = env.fixture_source("Test Game (USA, Europe) (Multiple Tracks).cue");
    let dump_cue = dump_directory.join("Test Game (USA, Europe) (CUE BIN).cue");
    let text = fs::read_to_string(&dat_cue).await.unwrap();
    fs::write(&dump_cue, format!("REM dumped by hand\r\n{text}"))
        .await
        .unwrap();
    let romfile_path = env
        .tmp_directory
        .join("Test Game (USA, Europe) (CUE BIN).ore");
    ore_rs::compress(
        &dump_cue,
        &romfile_path,
        &ore_rs::Options::default(),
        &[],
        &mut |_| {},
    )
    .unwrap();

    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    env.import_roms(&[&romfile_path]).await;

    // its tracks point to the ORE, its cue's ROM to nothing yet
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 2);
    assert!(roms.iter().all(|rom| rom.name.ends_with(".bin")));
    let romfiles = env.romfiles().await;
    assert_eq!(romfiles.len(), 1);
    let ore_romfile = romfiles.first().unwrap();

    // when the DAT's cue sheet is imported
    let cue_path = env
        .fixture("Test Game (USA, Europe) (Multiple Tracks).cue")
        .await;
    env.import_roms(&[&cue_path]).await;

    // then it is in the ORE, which every ROM points to
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 3);
    assert!(
        roms.iter()
            .all(|rom| rom.romfile_id == Some(ore_romfile.id))
    );
    assert_eq!(env.romfiles().await.len(), 1);
    assert!(!cue_path.exists());
    assert!(
        !system_directory
            .join("Test Game (USA, Europe) (CUE BIN).cue")
            .exists()
    );

    // and the ORE gives back the DAT's cue sheet
    env.run(&["check-roms", "-a"]).await.unwrap();
    let romfiles = env.romfiles().await;
    assert_eq!(romfiles.len(), 1);
    assert!(!romfiles[0].path.contains("/Trash/"));
}

use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given a game of two disks, from its archive
    let mut env = TestEnv::with_dat("Commodore - Amiga (20261125).dat").await;
    let archive = env.fixture("Test Game (Europe).7z").await;
    env.import_roms(&[&archive]).await;

    let system = env.system().await;
    assert_eq!(cartridge_platform(&system), Some("amiga"));
    let system_directory = env.system_directory(&system).await;

    // when
    env.run(&["convert-roms", "-f", "ORE", "-a", "-c"])
        .await
        .unwrap();

    // then each disk is an ORE of its own, a floppy's image taken whole
    let roms = env.roms_with_romfiles(&system).await;
    assert_eq!(roms.len(), 2);
    let romfiles = env.romfiles().await;
    assert_eq!(romfiles.len(), 2);
    for (rom, disk) in roms.iter().zip(1..) {
        let name = format!("Test Game (Europe) (Disk {disk})");
        assert_eq!(rom.name, format!("{name}.ipf"));
        let romfile = romfiles
            .iter()
            .find(|romfile| Some(romfile.id) == rom.romfile_id)
            .unwrap();
        let path = system_directory.join(format!("{name}.ore"));
        assert_eq!(romfile.path, env.stored_path(&path));
        let ore = ore_rs::Reader::open(&path).unwrap();
        assert_eq!(ore.platform(), "amiga");
        assert_eq!(ore.file_name(), Some(rom.name.as_str()));
        let disk = std::fs::read(env.fixture_source(&rom.name)).unwrap();
        let mut bytes = vec![0; disk.len()];
        ore_rs::Reader::open(&path)
            .unwrap()
            .read_at(0, &mut bytes)
            .unwrap();
        assert_eq!(bytes, disk);
    }
    assert!(!system_directory.join("Test Game (Europe).7z").exists());
    env.run(&["check-roms", "-a"]).await.unwrap();
}

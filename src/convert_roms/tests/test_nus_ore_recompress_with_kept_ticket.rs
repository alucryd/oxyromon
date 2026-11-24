use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given a title converted to ORE without its ticket, which no DAT lists
    let mut env = TestEnv::with_dat("Test System (20261025) (CDN No Ticket).dat").await;

    let fixture = env.fixture_source("Test Game (USA) (Update)");
    let update_directory = env.tmp_directory.join("Test Game (USA) (Update)");
    std::fs::create_dir(&update_directory).unwrap();
    for entry in std::fs::read_dir(&fixture).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().unwrap() != "cetk.48" {
            std::fs::copy(&path, update_directory.join(path.file_name().unwrap())).unwrap();
        }
    }
    env.run(&["import-roms", update_directory.to_str().unwrap()])
        .await
        .unwrap();
    let matches = subcommand().get_matches_from(["convert-roms", "-f", "ORE", "-a"]);
    main(&mut env.connection, &matches, &env.progress_bar)
        .await
        .unwrap();

    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;
    let ore = system_directory.join("Test Game (USA) (Update).ore");
    let (tmp_directory, progress_bar) = (env.tmp_directory.clone(), env.progress_bar.clone());
    let extracted = |name: &str| {
        let folder = tmp_directory.join(name);
        let progress_bar = progress_bar.clone();
        let ore = ore.clone();
        async move {
            CommonRomfile::from_path(&ore)
                .unwrap()
                .as_ore()
                .unwrap()
                .extract_title(&progress_bar, &folder)
                .await
                .unwrap();
            folder.join("title.tik")
        }
    };
    assert!(!extracted("before").await.exists());

    // and its ticket, kept since
    let ticket = std::fs::read(fixture.join("cetk.48")).unwrap();
    let tickets = env.tmp_directory.join("wiiu_tickets");
    std::fs::create_dir_all(&tickets).unwrap();
    std::fs::write(tickets.join("0005000e10101010.tik"), &ticket).unwrap();

    // when
    let matches =
        subcommand().get_matches_from(["convert-roms", "-f", "ORE", "-a", "-c", "--recompress"]);
    main(&mut env.connection, &matches, &env.progress_bar)
        .await
        .unwrap();

    // then the ORE, written again in its place, holds the ticket
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);
    assert_eq!(
        romfiles[0].path,
        ore.strip_prefix(&env.rom_directory)
            .unwrap()
            .to_str()
            .unwrap()
    );
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 5);
    assert!(
        roms.iter()
            .all(|rom| rom.romfile_id == Some(romfiles[0].id))
    );
    assert_eq!(std::fs::read(extracted("after").await).unwrap(), ticket);
}

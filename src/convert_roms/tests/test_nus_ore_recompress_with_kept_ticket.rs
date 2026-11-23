use crate::import_dats;
use crate::import_roms;
use super::*;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

#[tokio::test]
async fn test() {
    // given a title converted to ORE without its ticket, which no DAT lists
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = crate::config::TestDatabase::new();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    let tmp_directory =
        set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand().get_matches_from([
        "import-dats",
        "tests/Test System (20261025) (CDN No Ticket).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();
    let fixture = test_directory.join("Test Game (USA) (Update)");
    let update_directory = tmp_directory.join("Test Game (USA) (Update)");
    std::fs::create_dir(&update_directory).unwrap();
    for entry in std::fs::read_dir(&fixture).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().unwrap() != "cetk.48" {
            std::fs::copy(&path, update_directory.join(path.file_name().unwrap())).unwrap();
        }
    }
    let matches = import_roms::subcommand()
        .get_matches_from(["import-roms", update_directory.to_str().unwrap()]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();
    let matches = subcommand().get_matches_from(["convert-roms", "-f", "ORE", "-a"]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();
    let ore = system_directory.join("Test Game (USA) (Update).ore");
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
    let tickets = tmp_directory.join("wiiu_tickets");
    std::fs::create_dir_all(&tickets).unwrap();
    std::fs::write(tickets.join("0005000e10101010.tik"), &ticket).unwrap();

    // when
    let matches =
        subcommand().get_matches_from(["convert-roms", "-f", "ORE", "-a", "-c", "--recompress"]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then the ORE, written again in its place, holds the ticket
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);
    assert_eq!(
        romfiles[0].path,
        ore.strip_prefix(&rom_directory).unwrap().to_str().unwrap()
    );
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), 5);
    assert!(
        roms.iter()
            .all(|rom| rom.romfile_id == Some(romfiles[0].id))
    );
    assert_eq!(std::fs::read(extracted("after").await).unwrap(), ticket);
}

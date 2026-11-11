use super::super::config::{MUTEX, set_rom_directory, set_tmp_directory};
use super::super::import_dats;
use super::*;
use std::io::Write;
use tempfile::{NamedTempFile, TempDir};

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
    set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand().get_matches_from([
        "import-dats",
        "tests/Test System (20261111) (Clone Lists).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);

    // when
    let matches = subcommand().get_matches_from([
        "import-clone-lists",
        "tests/Test System (Clone Lists) (Redump).json",
    ]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then
    assert_eq!(
        find_clone_list_version_by_system_id(&mut connection, system.id)
            .await
            .as_deref(),
        Some("2026-11-11 00:00:00")
    );

    // each filter comes before its title; the compilation and the region-free
    // title are skipped
    let titles = find_clone_list_titles_by_system_id(&mut connection, system.id).await;
    let titles: Vec<(&str, &str, i64, i64, Option<&str>)> = titles
        .iter()
        .map(|title| {
            (
                title.group_name.as_str(),
                title.search_term.as_str(),
                title.name_type,
                title.priority,
                title.match_regions.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        titles,
        vec![
            ("Another Game", "Another Game (Premium Box)", 0, 1, None),
            ("Another Game", "Another Game", 0, 2, None),
            ("First Game (Japan)", "First Game", 0, 1, Some("Japan")),
            ("First Game", "First Game", 0, 1, None),
            ("Fishing Game", "Fishing Game", 0, 1, None),
            (
                "Fishing Game (Doukon Pack)",
                "Fishing Game (Doukon Pack)",
                0,
                1,
                None
            ),
            ("First Game", "Sequel Game", 0, 1, Some("Japan")),
            ("Sequel Game", "Sequel Game", 0, 1, None),
            ("Test Game", "Test Game", 0, 1, None),
            ("Test Game", "Testjuego.*", 2, 1, None),
            ("Test Game", "Testspiel (Germany)", 1, 1, None),
        ]
    );

    // when another list of the same version is imported
    let mut clone_list_file = NamedTempFile::new_in(tmp_directory.path()).unwrap();
    write!(
        clone_list_file,
        r#"{{"description": {{"name": "Test System (Clone Lists) (Redump)", "lastUpdated": "2026-11-11 00:00:00"}}, "variants": [{{"group": "Test Game", "titles": [{{"searchTerm": "Test Game"}}, {{"searchTerm": "Testspiel"}}]}}]}}"#
    )
    .unwrap();
    let imported = import_clone_list(
        &mut connection,
        &progress_bar,
        clone_list_file.path(),
        false,
    )
    .await
    .unwrap();

    // then it's skipped
    assert_eq!(imported, None);
    assert_eq!(
        find_clone_list_titles_by_system_id(&mut connection, system.id)
            .await
            .len(),
        11
    );

    // when it's forced
    let matches = subcommand().get_matches_from([
        String::from("import-clone-lists"),
        String::from("-f"),
        clone_list_file.path().to_str().unwrap().to_owned(),
    ]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then it replaces the previous one
    let titles = find_clone_list_titles_by_system_id(&mut connection, system.id).await;
    let search_terms: Vec<&str> = titles
        .iter()
        .map(|title| title.search_term.as_str())
        .collect();
    assert_eq!(search_terms, vec!["Test Game", "Testspiel"]);
}

use super::*;
use crate::test_support::TestEnv;
use std::io::Write;
use tempfile::NamedTempFile;

/// A stored title: its group, search term, name type, priority and conditions
type CloneListTitleRow<'a> = (
    &'a str,
    &'a str,
    i64,
    i64,
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
);

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261111) (Clone Lists).dat").await;

    let system = env.system().await;

    // when
    env.run(&[
        "import-clone-lists",
        "tests/Test System (Clone Lists) (Redump).json",
    ])
    .await
    .unwrap();

    // then
    assert_eq!(
        find_clone_list_version_by_system_id(&mut env.connection, system.id)
            .await
            .as_deref(),
        Some("2026-11-11 00:00:00")
    );

    // each filter comes before its title; the compilation, the region-free
    // title and the title whose regex doesn't compile are skipped, and so are
    // the filters that need a region order, change nothing or have a pattern
    // that doesn't compile
    let titles = find_clone_list_titles_by_system_id(&mut env.connection, system.id).await;
    let titles: Vec<CloneListTitleRow> = titles
        .iter()
        .map(|title| {
            (
                title.group_name.as_str(),
                title.search_term.as_str(),
                title.name_type,
                title.priority,
                title.match_regions.as_deref(),
                title.match_languages.as_deref(),
                title.match_string.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        titles,
        vec![
            (
                "Another Game",
                "Another Game (Premium Box)",
                0,
                1,
                None,
                None,
                None
            ),
            ("Another Game", "Another Game", 0, 2, None, None, None),
            (
                "First Game (Japan)",
                "First Game",
                0,
                1,
                Some("Japan"),
                None,
                None
            ),
            ("First Game", "First Game", 0, 1, None, None, None),
            ("Fishing Game", "Fishing Game", 0, 1, None, None, None),
            (
                "Fishing Game (Doukon Pack)",
                "Fishing Game (Doukon Pack)",
                0,
                1,
                None,
                None,
                None
            ),
            ("First Game", "Sequel Game", 0, 1, Some("Japan"), None, None),
            ("Sequel Game", "Sequel Game", 0, 1, None, None, None),
            ("Test Game", "Test Game", 0, 1, None, None, None),
            ("Test Game", "Testjuego.*", 2, 1, None, None, None),
            ("Test Game", "Testspiel (Germany)", 1, 1, None, None, None),
            (
                "Word Game (English)",
                "Word Game",
                0,
                1,
                None,
                Some("En"),
                None
            ),
            (
                "Word Game (Rev 1)",
                "Word Game",
                0,
                1,
                None,
                None,
                Some("Rev 1")
            ),
            ("Word Game", "Word Game", 0, 1, None, None, None),
        ]
    );

    // when another list of the same version is imported
    let mut clone_list_file = NamedTempFile::new_in(&env.tmp_directory).unwrap();
    write!(
        clone_list_file,
        r#"{{"description": {{"name": "Test System (Clone Lists) (Redump)", "lastUpdated": "2026-11-11 00:00:00"}}, "variants": [{{"group": "Test Game", "titles": [{{"searchTerm": "Test Game"}}, {{"searchTerm": "Testspiel"}}]}}]}}"#
    )
    .unwrap();
    let imported = import_clone_list(
        &mut env.connection,
        &env.progress_bar,
        clone_list_file.path(),
        false,
    )
    .await
    .unwrap();

    // then it's skipped
    assert_eq!(imported, None);
    assert_eq!(
        find_clone_list_titles_by_system_id(&mut env.connection, system.id)
            .await
            .len(),
        14
    );

    // when it's forced
    env.run(&[
        "import-clone-lists",
        "-f",
        clone_list_file.path().to_str().unwrap(),
    ])
    .await
    .unwrap();

    // then it replaces the previous one
    let titles = find_clone_list_titles_by_system_id(&mut env.connection, system.id).await;
    let search_terms: Vec<&str> = titles
        .iter()
        .map(|title| title.search_term.as_str())
        .collect();
    assert_eq!(search_terms, vec!["Test Game", "Testspiel"]);
}

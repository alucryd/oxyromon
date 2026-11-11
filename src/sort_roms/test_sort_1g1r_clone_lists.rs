use super::super::import_clone_lists;
use super::super::import_dats;
use super::*;
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

    let matches = import_clone_lists::subcommand().get_matches_from([
        "import-clone-lists",
        "tests/Test System (Clone Lists) (Redump).json",
    ]);
    import_clone_lists::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);

    let all_regions = vec![];
    let one_regions = vec![
        Region::UnitedStates,
        Region::Japan,
        Region::Germany,
        Region::Spain,
    ];

    // when
    sort_system(
        &mut connection,
        &progress_bar,
        true,
        false,
        &system,
        &all_regions,
        &one_regions,
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        true,
        &PreferredRegion::None,
        &PreferredVersion::None,
        &[],
        &None,
        &None,
        true,
        true,
    )
    .await
    .unwrap();

    // then
    let games = find_games_by_system_id(&mut connection, system.id).await;
    assert_eq!(11, games.len());

    let sortings: Vec<(&str, i64)> = games
        .iter()
        .map(|game| (game.name.as_str(), game.sorting))
        .collect();
    assert_eq!(
        sortings,
        vec![
            // the list prefers the Premium Box within its region
            ("Another Game (Japan)", Sorting::Ignored as i64),
            (
                "Another Game (Japan) (Premium Box)",
                Sorting::OneRegion as i64
            ),
            // the first game is a group of its own in Japan, where the sequel
            // is the first game elsewhere
            ("First Game (Japan)", Sorting::OneRegion as i64),
            ("First Game (USA)", Sorting::OneRegion as i64),
            // the list's title naming the Doukon Pack is more specific than
            // the one naming the game
            ("Fishing Game (Japan)", Sorting::OneRegion as i64),
            (
                "Fishing Game (Japan) (Doukon Pack)",
                Sorting::OneRegion as i64
            ),
            ("Sequel Game (Japan)", Sorting::Ignored as i64),
            ("Sequel Game (USA)", Sorting::OneRegion as i64),
            // and the Spanish and German releases are the same game under
            // other names, found by a regex and by their full names
            ("Test Game (USA)", Sorting::OneRegion as i64),
            ("Testjuego (Spain)", Sorting::Ignored as i64),
            ("Testspiel (Germany)", Sorting::Ignored as i64),
        ]
    );
}

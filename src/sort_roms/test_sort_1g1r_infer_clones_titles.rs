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
        "tests/Test System (20261110) (Infer Clones).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);

    let all_regions = vec![];
    let one_regions = vec![Region::UnitedStates, Region::Europe];

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
    assert_eq!(7, games.len());

    let sortings: Vec<(&str, i64)> = games
        .iter()
        .map(|game| (game.name.as_str(), game.sorting))
        .collect();
    assert_eq!(
        sortings,
        vec![
            ("Another Game (USA)", Sorting::OneRegion as i64),
            ("Another Game (USA) (Premium Box)", Sorting::Ignored as i64),
            (
                "Another Game (USA) (Premium Pack Bonus Disc)",
                Sorting::OneRegion as i64
            ),
            ("Test Game (Europe) (Disc 1)", Sorting::Ignored as i64),
            ("Test Game (Europe) (Disc 2)", Sorting::Ignored as i64),
            ("Test Game (USA) (Disc 1)", Sorting::OneRegion as i64),
            ("Test Game (USA) (Disc 2)", Sorting::OneRegion as i64),
        ]
    );
}

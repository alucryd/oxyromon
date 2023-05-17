use super::super::database::*;
use super::super::import_dats;
use super::*;
use tempfile::{NamedTempFile, TempDir};

#[async_std::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(&test_directory).unwrap();
    set_rom_directory(PathBuf::from(rom_directory.path()));
    let tmp_directory = TempDir::new_in(&test_directory).unwrap();
    set_tmp_directory(PathBuf::from(tmp_directory.path()));

    let matches = import_dats::subcommand().get_matches_from(&[
        "import-dats",
        "tests/Test System (20200721) (Parent-Clone).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);

    let matches = subcommand().get_matches_from(&["config", "-y"]);
    let all_regions = vec![];
    let one_regions = vec![Region::UnitedStates, Region::Europe];

    // when
    sort_system(
        &mut connection,
        &matches,
        &progress_bar,
        &system,
        &all_regions,
        &one_regions,
        &[],
        &[],
        &[],
        true,
        &PreferRegion::None,
        &PreferVersion::None,
        &[],
        &SubfolderScheme::None,
        &SubfolderScheme::None,
        false,
    )
    .await
    .unwrap();

    // then
    let games = find_games_by_system_id(&mut connection, system.id).await;
    assert_eq!(6, games.len());

    let one_regions_indices = vec![0, 1, 4];
    let trash_indices = vec![2, 3, 5];

    for i in one_regions_indices {
        assert_eq!(games[i].sorting, Sorting::OneRegion as i64);
    }

    for i in trash_indices {
        assert_eq!(games[i].sorting, Sorting::Ignored as i64);
    }
}

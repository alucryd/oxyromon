use super::*;
use tempfile::NamedTempFile;

#[tokio::test]
async fn test() {
    // given
    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();
    let progress_bar = get_progress_bar(0, get_none_progress_style());

    let key = "REGIONS_ONE";

    add_to_list(&mut connection, &progress_bar, key, "US", None).await;
    let matches = subcommand().get_matches_from(["sort-roms", "-y"]);

    // when
    let all_regions = get_regions(&mut connection, &matches, key, None).await;

    // then
    assert_eq!(all_regions.len(), 1);
    assert_eq!(all_regions.first().unwrap(), &Region::UnitedStates);
}

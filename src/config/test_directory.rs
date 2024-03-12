use super::*;
use tempfile::NamedTempFile;

#[tokio::test]
async fn test() {
    // given
    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let test_directory = get_canonicalized_path(&String::from("tests"))
        .await
        .unwrap();
    let key = "TEST_DIRECTORY";

    // when
    set_directory(&mut connection, key, &test_directory).await;

    let directory = get_directory(&mut connection, key).await.unwrap();

    // then
    assert_eq!(directory, test_directory);
}

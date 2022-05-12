use super::*;
use tempfile::NamedTempFile;

#[async_std::test]
async fn test() {
    // given
    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let key = "TEST_BOOLEAN";

    // when
    set_bool(&mut connection, key, true).await;
    let bool = get_bool(&mut connection, key).await;

    // then
    assert_eq!(bool, true);
}

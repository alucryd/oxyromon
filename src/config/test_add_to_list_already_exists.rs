use super::*;
use tempfile::NamedTempFile;

#[tokio::test]
async fn test() {
    // given
    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let key = "DISCARD_FLAGS";

    set_list(&mut connection, key, &[String::from("item1")]).await;

    // when
    add_to_list(&mut connection, key, "item1").await;
    let list = get_list(&mut connection, key).await;

    // then
    assert_eq!(list.len(), 1);
    assert_eq!(list.first().unwrap(), "item1");
}

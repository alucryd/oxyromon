use super::*;
use tempfile::NamedTempFile;

#[tokio::test]
async fn test() {
    // given
    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();
    let progress_bar = get_progress_bar(0, get_none_progress_style());

    let key = "DISCARD_FLAGS";

    set_list(&mut connection, key, &[String::from("item1")]).await;

    // when
    remove_from_list(&mut connection, &progress_bar, key, "item2").await;
    let list = get_list(&mut connection, key).await;

    // then
    assert_eq!(list.len(), 1);
    assert_eq!(list.first().unwrap(), "item1");
}

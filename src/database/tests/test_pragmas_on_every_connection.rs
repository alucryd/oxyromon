use super::*;
use crate::config::TestDatabase;

#[tokio::test]
async fn test() {
    // given
    let db = TestDatabase::new();
    let pool = establish_connection(db.path().to_str().unwrap()).await;

    // when
    // hold as many connections as the pool allows, so that it has to open all of them; only
    // server and mcp builds allow more than one, so only they can catch a connection left out
    let mut connections = Vec::new();
    for _ in 0..pool.options().get_max_connections() {
        connections.push(pool.acquire().await.unwrap());
    }

    // then
    for connection in &mut connections {
        let temp_store: i64 = sqlx::query_scalar("PRAGMA temp_store")
            .fetch_one(&mut **connection)
            .await
            .unwrap();
        assert_eq!(temp_store, 2);
        let cache_size: i64 = sqlx::query_scalar("PRAGMA cache_size")
            .fetch_one(&mut **connection)
            .await
            .unwrap();
        assert_eq!(cache_size, 10000);
        let mmap_size: i64 = sqlx::query_scalar("PRAGMA mmap_size")
            .fetch_one(&mut **connection)
            .await
            .unwrap();
        assert_eq!(mmap_size, 268435456);
        let synchronous: i64 = sqlx::query_scalar("PRAGMA synchronous")
            .fetch_one(&mut **connection)
            .await
            .unwrap();
        assert_eq!(synchronous, 1);
        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&mut **connection)
            .await
            .unwrap();
        assert_eq!(foreign_keys, 1);
        let locking_mode: String = sqlx::query_scalar("PRAGMA locking_mode")
            .fetch_one(&mut **connection)
            .await
            .unwrap();
        if cfg!(any(feature = "server", feature = "mcp")) {
            assert_eq!(locking_mode, "normal");
        } else {
            assert_eq!(locking_mode, "exclusive");
        }
    }
}

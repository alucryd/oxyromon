use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    // when
    env.run(&["import-dats", "tests/Test System (20261025) (CDN).dat"])
        .await
        .unwrap();

    // then a NUS title's contents, named by their bare ID, are original, as
    // are its TMD, ticket and H3 file
    let roms = find_roms(&mut env.connection).await;
    assert_eq!(roms.len(), 6);
    for rom in &roms {
        assert!(rom.original, "{}", rom.name);
    }
}

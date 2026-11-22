use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721) (MAME Rebuild).dat").await;

    // when rebuilding to the merging strategy the system already uses
    env.run(&["rebuild-roms", "--all", "-m", "SPLIT"])
        .await
        .unwrap();

    // then nothing changed
    let system = find_arcade_systems(&mut env.connection).await.remove(0);
    assert_eq!(system.merging, Merging::Split as i64);
    let romfiles = find_romfiles(&mut env.connection).await;
    assert!(romfiles.is_empty());
}

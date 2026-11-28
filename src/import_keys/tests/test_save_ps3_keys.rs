use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    let env = TestEnv::new().await;
    // given a data directory not made yet
    let dir = env.tmp_directory.join("data");
    let (a, b) = ([0x11; 16], [0x22; 16]);

    // when an IRD's key is kept, then another's and the first again
    assert_eq!(save_ps3_keys(&dir, &[a]).unwrap(), 1);
    assert_eq!(save_ps3_keys(&dir, &[b, a]).unwrap(), 1);

    // then each is kept once, in the order met, as import-keys reads them
    assert_eq!(load_ps3_keys(&dir.join(PS3_KEYS_FILE)).unwrap(), [a, b]);
}

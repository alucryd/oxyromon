use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    let nsz_path = env.fixture("Test Game (USA).nsz").await;
    let nsz = CommonRomfile::from_path(&nsz_path)
        .unwrap()
        .as_nsz()
        .unwrap();

    // when
    let (sha1, sha1_size) = nsz
        .get_hash_and_size(
            &mut env.connection,
            &env.progress_bar,
            1,
            1,
            &HashAlgorithm::Sha1,
        )
        .await
        .unwrap();
    let (md5, md5_size) = nsz
        .get_hash_and_size(
            &mut env.connection,
            &env.progress_bar,
            1,
            1,
            &HashAlgorithm::Md5,
        )
        .await
        .unwrap();

    // then
    // an NSZ is matched against the DB by its decompressed NSP
    assert_eq!(sha1, "8dfa6a7045f8bd56f8c5d33c20a48b07659ecb76");
    assert_eq!(md5, "bbddf6ccb6451061eaa16c2de22dc469");
    assert_eq!(sha1_size, 40946);
    assert_eq!(md5_size, 40946);
}

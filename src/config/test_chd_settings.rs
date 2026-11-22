use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;

    for media in ["CD", "DVD", "HD", "LD"] {
        let hunk_size = format!("CHD_{media}_HUNK_SIZE");
        let algorithms = format!("CHD_{media}_COMPRESSION_ALGORITHMS");

        // when
        set_string(&mut env.connection, &hunk_size, "4096", None).await;
        unset_setting(&mut env.connection, &env.progress_bar, &hunk_size, None)
            .await
            .unwrap();
        set_list(
            &mut env.connection,
            &algorithms,
            &[String::from("none")],
            None,
        )
        .await;
        unset_setting(&mut env.connection, &env.progress_bar, &algorithms, None)
            .await
            .unwrap();

        // then: back to auto
        assert_eq!(
            get_string(&mut env.connection, &hunk_size, None).await,
            None
        );
        assert!(
            get_list(&mut env.connection, &algorithms, None)
                .await
                .is_empty()
        );
    }

    // when: the codec order is kept as given, like CD and DVD
    let key = "CHD_HD_COMPRESSION_ALGORITHMS";
    add_to_list(&mut env.connection, &env.progress_bar, key, "zstd", None).await;
    add_to_list(&mut env.connection, &env.progress_bar, key, "lzma", None).await;

    // then
    assert_eq!(
        get_list(&mut env.connection, key, None).await,
        ["zstd", "lzma"]
    );
}

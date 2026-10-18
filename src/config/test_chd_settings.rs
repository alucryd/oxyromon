use super::*;
use tempfile::NamedTempFile;

#[tokio::test]
async fn test() {
    // given
    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();
    let progress_bar = get_progress_bar(0, get_none_progress_style());

    for media in ["CD", "DVD", "HD", "LD"] {
        let hunk_size = format!("CHD_{media}_HUNK_SIZE");
        let algorithms = format!("CHD_{media}_COMPRESSION_ALGORITHMS");

        // when
        set_string(&mut connection, &hunk_size, "4096", None).await;
        unset_setting(&mut connection, &progress_bar, &hunk_size, None)
            .await
            .unwrap();
        set_list(&mut connection, &algorithms, &[String::from("none")], None).await;
        unset_setting(&mut connection, &progress_bar, &algorithms, None)
            .await
            .unwrap();

        // then: back to auto
        assert_eq!(get_string(&mut connection, &hunk_size, None).await, None);
        assert!(
            get_list(&mut connection, &algorithms, None)
                .await
                .is_empty()
        );
    }

    // when: the codec order is kept as given, like CD and DVD
    let key = "CHD_HD_COMPRESSION_ALGORITHMS";
    add_to_list(&mut connection, &progress_bar, key, "zstd", None).await;
    add_to_list(&mut connection, &progress_bar, key, "lzma", None).await;

    // then
    assert_eq!(get_list(&mut connection, key, None).await, ["zstd", "lzma"]);
}

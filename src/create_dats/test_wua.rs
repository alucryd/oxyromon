use super::super::config::*;
use super::super::database::*;
use super::*;
use tempfile::{NamedTempFile, TempDir};

#[tokio::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    let system_directory = tmp_directory.path().join("Test System");
    std::fs::create_dir(&system_directory).unwrap();
    // a lossless WUA, its update partition in the store beside it
    let keys = wux_rs::load_keys(&test_directory.join("Test Game (USA).keys.txt")).unwrap();
    wux_rs::to_wua(
        &[&test_directory.join("Test Game (USA).wux")],
        &system_directory.join("Test Game (USA).wua"),
        &keys,
        Some(&system_directory.join(wux_rs::STORE_DIR)),
        &mut |_| {},
    )
    .unwrap();
    let dat_directory = tmp_directory.path().join("DAT");

    // when
    create_dat(
        &mut connection,
        &progress_bar,
        &system_directory,
        Some(&dat_directory),
        None,
        None,
        Some(&String::from("42")),
        None,
        None,
    )
    .await
    .unwrap();

    // then the WUA is listed as the disc it holds, and its store is not a game
    let dat = std::fs::read_to_string(dat_directory.join("Test System (42).dat")).unwrap();
    assert!(dat.contains(
        r#"<rom name="Test Game (USA).iso" size="1867776" crc="d2066103" md5="286d9857c9a92545a989267065423ed6" sha1="1adab7c09cc400e9f07a31abed691a0965ff66df"/>"#
    ), "{dat}");
    assert_eq!(dat.matches("<game ").count(), 1, "{dat}");
}

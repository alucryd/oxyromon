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
    std::fs::copy(
        test_directory.join("Test Game (USA).wux"),
        system_directory.join("Test Game (USA).wux"),
    )
    .unwrap();
    let update_directory = system_directory.join("Test Game (USA) (Update)");
    std::fs::create_dir(&update_directory).unwrap();
    for entry in std::fs::read_dir(test_directory.join("Test Game (USA) (Update)")).unwrap() {
        let path = entry.unwrap().path();
        std::fs::copy(&path, update_directory.join(path.file_name().unwrap())).unwrap();
    }
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

    // then the WUX is listed as the WUD it holds, as in the WUD DAT
    let dat = std::fs::read_to_string(dat_directory.join("Test System (42).dat")).unwrap();
    assert!(dat.contains(
        r#"<rom name="Test Game (USA).wud" size="720896" crc="0274c8e0" md5="d31c6a655ad789717f43e497f71f7630" sha1="39e947c319b9070a05e736e0d827bacdbce3f072"/>"#
    ), "{dat}");
    // and the update's folder is one game, its files its ROMs, as in the CDN DAT
    assert_eq!(dat.matches("<game ").count(), 2);
    assert!(dat.contains(r#"<game name="Test Game (USA) (Update)">"#));
    let cdn =
        std::fs::read_to_string(test_directory.join("Test System (20261025) (CDN).dat")).unwrap();
    for line in cdn.lines().filter(|line| line.contains("<rom ")) {
        assert!(dat.contains(line.trim()), "{line}");
    }
}

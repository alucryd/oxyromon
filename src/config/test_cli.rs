use super::*;
use std::path::PathBuf;
use tempfile::{NamedTempFile, TempDir};

#[tokio::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();
    let rom_directory = TempDir::new_in("tests").unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;

    let pb = ProgressBar::hidden();

    // SET a boolean
    let m = subcommand().get_matches_from(["config", "--set", "PREFER_PARENTS", "true"]);
    main(&mut connection, &m, &pb).await.unwrap();
    assert!(get_bool(&mut connection, "PREFER_PARENTS", None).await);

    // SET a valid choice
    let m = subcommand().get_matches_from(["config", "--set", "PREFER_REGIONS", "broad"]);
    main(&mut connection, &m, &pb).await.unwrap();
    assert_eq!(
        get_string(&mut connection, "PREFER_REGIONS", None).await.as_deref(),
        Some("broad")
    );

    // SET an invalid choice is rejected, leaving the previous value untouched
    let m = subcommand().get_matches_from(["config", "--set", "PREFER_REGIONS", "bogus"]);
    main(&mut connection, &m, &pb).await.unwrap();
    assert_eq!(
        get_string(&mut connection, "PREFER_REGIONS", None).await.as_deref(),
        Some("broad")
    );

    // SET a list directly is rejected
    let m = subcommand().get_matches_from(["config", "--set", "REGIONS_ALL", "us"]);
    main(&mut connection, &m, &pb).await.unwrap();
    assert!(
        !get_list(&mut connection, "REGIONS_ALL", None)
            .await
            .contains(&"us".to_string())
    );

    // ADD then read back
    let m = subcommand().get_matches_from(["config", "--add", "REGIONS_ALL", "us"]);
    main(&mut connection, &m, &pb).await.unwrap();
    assert!(
        get_list(&mut connection, "REGIONS_ALL", None)
            .await
            .contains(&"us".to_string())
    );

    // ADD an invalid value to a choice-list is rejected
    let m = subcommand().get_matches_from(["config", "--add", "REGIONS_ONE_ARCADE", "bogus"]);
    main(&mut connection, &m, &pb).await.unwrap();
    assert!(
        !get_list(&mut connection, "REGIONS_ONE_ARCADE", None)
            .await
            .contains(&"bogus".to_string())
    );

    // REMOVE
    let m = subcommand().get_matches_from(["config", "--remove", "REGIONS_ALL", "us"]);
    main(&mut connection, &m, &pb).await.unwrap();
    assert!(
        !get_list(&mut connection, "REGIONS_ALL", None)
            .await
            .contains(&"us".to_string())
    );

    // UNSET a nullable list
    let m = subcommand().get_matches_from(["config", "--add", "REGIONS_ALL", "eu"]);
    main(&mut connection, &m, &pb).await.unwrap();
    let m = subcommand().get_matches_from(["config", "--unset", "REGIONS_ALL"]);
    main(&mut connection, &m, &pb).await.unwrap();
    assert!(get_list(&mut connection, "REGIONS_ALL", None).await.is_empty());

    // UNSET a non-nullable setting is rejected, value preserved
    let m = subcommand().get_matches_from(["config", "--unset", "PREFER_PARENTS"]);
    main(&mut connection, &m, &pb).await.unwrap();
    assert!(get_bool(&mut connection, "PREFER_PARENTS", None).await);

    // GET and LIST just run without error
    let m = subcommand().get_matches_from(["config", "--get", "PREFER_PARENTS"]);
    main(&mut connection, &m, &pb).await.unwrap();
    let m = subcommand().get_matches_from(["config", "--list"]);
    main(&mut connection, &m, &pb).await.unwrap();

    // SET an unknown key is an error path but returns Ok
    let m = subcommand().get_matches_from(["config", "--set", "NOT_A_SETTING", "x"]);
    main(&mut connection, &m, &pb).await.unwrap();
}

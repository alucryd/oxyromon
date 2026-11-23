use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;

    // SET a boolean
    env.run(&["config", "--set", "PREFER_PARENTS", "true"])
        .await
        .unwrap();
    assert!(get_bool(&mut env.connection, "PREFER_PARENTS", None).await);

    // SET a valid choice
    env.run(&["config", "--set", "PREFER_REGIONS", "broad"])
        .await
        .unwrap();
    assert_eq!(
        get_string(&mut env.connection, "PREFER_REGIONS", None)
            .await
            .as_deref(),
        Some("broad")
    );

    // SET an invalid choice is rejected, leaving the previous value untouched
    env.run(&["config", "--set", "PREFER_REGIONS", "bogus"])
        .await
        .unwrap();
    assert_eq!(
        get_string(&mut env.connection, "PREFER_REGIONS", None)
            .await
            .as_deref(),
        Some("broad")
    );

    // SET a list directly is rejected
    env.run(&["config", "--set", "REGIONS_ALL", "us"])
        .await
        .unwrap();
    assert!(
        !get_list(&mut env.connection, "REGIONS_ALL", None)
            .await
            .contains(&"us".to_string())
    );

    // ADD then read back
    env.run(&["config", "--add", "REGIONS_ALL", "us"])
        .await
        .unwrap();
    assert!(
        get_list(&mut env.connection, "REGIONS_ALL", None)
            .await
            .contains(&"us".to_string())
    );

    // ADD an invalid value to a choice-list is rejected
    env.run(&["config", "--add", "REGIONS_ONE_ARCADE", "bogus"])
        .await
        .unwrap();
    assert!(
        !get_list(&mut env.connection, "REGIONS_ONE_ARCADE", None)
            .await
            .contains(&"bogus".to_string())
    );

    // REMOVE
    env.run(&["config", "--remove", "REGIONS_ALL", "us"])
        .await
        .unwrap();
    assert!(
        !get_list(&mut env.connection, "REGIONS_ALL", None)
            .await
            .contains(&"us".to_string())
    );

    // UNSET a nullable list
    env.run(&["config", "--add", "REGIONS_ALL", "eu"])
        .await
        .unwrap();
    env.run(&["config", "--unset", "REGIONS_ALL"])
        .await
        .unwrap();
    assert!(
        get_list(&mut env.connection, "REGIONS_ALL", None)
            .await
            .is_empty()
    );

    // UNSET a non-nullable setting is rejected, value preserved
    env.run(&["config", "--unset", "PREFER_PARENTS"])
        .await
        .unwrap();
    assert!(get_bool(&mut env.connection, "PREFER_PARENTS", None).await);

    // GET and LIST just run without error
    env.run(&["config", "--get", "PREFER_PARENTS"])
        .await
        .unwrap();
    env.run(&["config", "--list"]).await.unwrap();

    // SET an unknown key is an error path but returns Ok
    env.run(&["config", "--set", "NOT_A_SETTING", "x"])
        .await
        .unwrap();
}

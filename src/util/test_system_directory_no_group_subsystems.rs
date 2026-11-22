use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    set_bool(&mut env.connection, "GROUP_SUBSYSTEMS", false, None).await;

    let system = System {
        id: 1,
        name: String::from("Nintendo - Nintendo 64 (BigEndian)"),
        custom_name: None,
        description: String::from(""),
        version: String::from(""),
        url: None,
        arcade: false,
        completion: 0,
        merging: Merging::NonMerged as i64,
        custom_extension: None,
        games_complete: 0,
        games_total: 0,
    };

    // when
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();

    // then
    assert_eq!(
        system_directory.file_name().unwrap().to_str().unwrap(),
        "Nintendo - Nintendo 64 (BigEndian)"
    );
}

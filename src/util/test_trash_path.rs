use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    let system = System {
        id: 1,
        name: String::from("Test System"),
        custom_name: None,
        description: String::from(""),
        version: String::from(""),
        url: None,
        arcade: true,
        completion: 0,
        merging: Merging::NonMerged as i64,
        custom_extension: None,
        games_complete: 0,
        games_total: 0,
    };
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();
    let trash_directory = get_trash_directory(&mut env.connection, Some(&system))
        .await
        .unwrap();

    let parent_path = system_directory.join("1944").join("c83u.p3");
    let clone_path = system_directory.join("1944d").join("c83u.p3");
    let other_clone_path = system_directory.join("1944j").join("c83u.p3");

    // when the trash holds no file of that name
    let parent = get_trash_path(&mut env.connection, Some(&system), &parent_path)
        .await
        .unwrap();

    // then it goes under its file name
    assert_eq!(parent, trash_directory.join("c83u.p3"));

    // when another game's file of the same name is already there
    fs::create_dir_all(&trash_directory).await.unwrap();
    fs::write(&parent, b"").await.unwrap();
    let clone = get_trash_path(&mut env.connection, Some(&system), &clone_path)
        .await
        .unwrap();

    // then it keeps its game directory
    assert_eq!(clone, trash_directory.join("1944d").join("c83u.p3"));

    // when that place is taken too
    fs::create_dir_all(clone.parent().unwrap()).await.unwrap();
    fs::write(&clone, b"").await.unwrap();
    let taken = get_trash_path(
        &mut env.connection,
        Some(&system),
        &system_directory.join("1944d").join("c83u.p3"),
    )
    .await
    .unwrap();

    // then it is numbered rather than overwriting it
    assert_eq!(taken, trash_directory.join("1944d").join("c83u (2).p3"));

    // and files of other games stay apart
    assert_eq!(
        get_trash_path(&mut env.connection, Some(&system), &other_clone_path)
            .await
            .unwrap(),
        trash_directory.join("1944j").join("c83u.p3")
    );

    // and a file already in the trash stays where it is
    assert_eq!(
        get_trash_path(&mut env.connection, Some(&system), &clone)
            .await
            .unwrap(),
        clone
    );
}

use super::*;

#[tokio::test]
async fn test() {
    // given
    let test_directory = Path::new("tests");
    let game = Game {
        id: 1,
        name: String::from("game name"),
        description: String::from(""),
        comment: None,
        external_id: None,
        device: false,
        bios: false,
        jbfolder: false,
        regions: String::from(""),
        sorting: Sorting::AllRegions as i64,
        complete: false,
        system_id: 1,
        parent_id: None,
        bios_id: None,
        playlist_id: None,
    };

    // when
    let path = compute_new_playlist_path(&game, &test_directory, &SubfolderScheme::Alpha)
        .await
        .unwrap();

    // then
    assert_eq!(path, test_directory.join("G/game name.m3u"));
}

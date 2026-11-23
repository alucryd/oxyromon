use super::*;
use crate::prompt::SCRIPTED_SELECTIONS;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    // both clones have this file, so importing it asks which one it is
    env.import_dats(&["Test System (20200721) (MAME Merged).dat"])
        .await;
    let romfile_path = env.tmp_directory.join("Unknown.rom");
    fs::copy(
        env.fixture_source("Test Game (USA, Europe) (Beta).rom"),
        &romfile_path,
    )
    .await
    .unwrap();

    // when picking "None"
    SCRIPTED_SELECTIONS.lock().unwrap().push_back(Some(0));
    let result = env
        .run(&["import-roms", romfile_path.to_str().unwrap()])
        .await;
    let unanswered = SCRIPTED_SELECTIONS.lock().unwrap().drain(..).count();

    // then the file is skipped
    result.unwrap();
    assert_eq!(unanswered, 0, "the ROM prompt was never reached");
    assert!(romfile_path.is_file());
    assert!(find_romfiles(&mut env.connection).await.is_empty());
}

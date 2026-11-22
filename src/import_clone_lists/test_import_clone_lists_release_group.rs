use super::*;
use crate::test_support::TestEnv;
use std::io::Write;
use tempfile::NamedTempFile;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    // a Redump DAT
    env.import_dats(&["Test System (20261111) (Clone Lists).dat"])
        .await;

    let system = env.system().await;

    // and a No-Intro clone list for a system of the same name
    let mut clone_list_file = NamedTempFile::new_in(env.tmp_directory).unwrap();
    write!(
        clone_list_file,
        r#"{{"description": {{"name": "Test System (Clone Lists) (No-Intro)", "lastUpdated": "2026-11-11 00:00:00"}}, "variants": [{{"group": "Test Game", "titles": [{{"searchTerm": "Test Game"}}, {{"searchTerm": "Testspiel"}}]}}]}}"#
    )
    .unwrap();

    // when
    let imported = import_clone_list(
        &mut env.connection,
        &env.progress_bar,
        clone_list_file.path(),
        false,
    )
    .await
    .unwrap();

    // then
    assert_eq!(imported, None);
    assert!(
        find_clone_list_version_by_system_id(&mut env.connection, system.id)
            .await
            .is_none()
    );
}

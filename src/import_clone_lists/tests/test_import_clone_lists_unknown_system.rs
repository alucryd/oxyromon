use super::*;
use crate::test_support::TestEnv;
use std::io::Write;
use tempfile::NamedTempFile;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    let mut clone_list_file = NamedTempFile::new_in(env.tmp_directory).unwrap();
    write!(
        clone_list_file,
        r#"{{"description": {{"name": "Unknown System (Redump)", "lastUpdated": "2026-11-11 00:00:00"}}, "variants": []}}"#
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
}

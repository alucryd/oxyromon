use super::*;
use tempfile::TempDir;

#[test]
fn test() {
    // given
    let directory = TempDir::new_in("tests").unwrap();
    let progress_bar = ProgressBar::hidden();
    let destination = directory.path().join(WIIU_KEYS_FILE);
    let a = directory.path().join("a.txt");
    let b = directory.path().join("b.txt");
    std::fs::write(
        &a,
        "# Cemu's format\n11111111111111111111111111111111 # Some Game\n2222-2222-2222-2222-2222-2222-2222-2222\n",
    )
    .unwrap();
    std::fs::write(
        &b,
        "22222222222222222222222222222222\n33333333333333333333333333333333\nnot a key\n",
    )
    .unwrap();

    // when
    import_keys(&progress_bar, &[&a], &destination).unwrap();
    import_keys(&progress_bar, &[&b, &a], &destination).unwrap();

    // then each key is kept once, in the order first seen
    assert_eq!(
        std::fs::read_to_string(&destination).unwrap(),
        "11111111111111111111111111111111\n\
         22222222222222222222222222222222\n\
         33333333333333333333333333333333\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&destination)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    assert!(!destination.with_extension("txt.part").exists());
}

use super::*;

#[test]
fn test() {
    // a PUP's version.txt, as it reads
    assert_eq!(ps3_update_directory("4.90\n"), Path::new("PS3_UPDATE/4.90"));
    // a crafted one cannot climb out, nor name the folder itself
    assert_eq!(
        ps3_update_directory("../../etc"),
        Path::new("PS3_UPDATE/etc")
    );
    assert_eq!(
        ps3_update_directory("/abs/path"),
        Path::new("PS3_UPDATE/abspath")
    );
    assert_eq!(
        ps3_update_directory("...."),
        Path::new("PS3_UPDATE/unknown")
    );
    assert_eq!(ps3_update_directory(""), Path::new("PS3_UPDATE/unknown"));
    for version in ["../x", "..", "a/../../b", "C:\\x", "\0"] {
        let directory = ps3_update_directory(version);
        assert_eq!(directory.components().count(), 2, "{version:?}");
        assert!(
            directory
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_))),
            "{version:?}"
        );
    }
}

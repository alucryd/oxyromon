use super::*;

#[test]
fn test() {
    assert_eq!(
        split_release_group("Sony - PlayStation (Redump)"),
        ("Sony - PlayStation", Some("Redump"))
    );
    assert_eq!(
        split_release_group("Nintendo - Nintendo 3DS (Digital) (CDN) (No-Intro)"),
        ("Nintendo - Nintendo 3DS (Digital) (CDN)", Some("No-Intro"))
    );
    assert_eq!(split_release_group("Test System"), ("Test System", None));

    assert_eq!(get_release_group("http://redump.org/"), Some("Redump"));
    assert_eq!(get_release_group("www.no-intro.org"), Some("No-Intro"));
    assert_eq!(get_release_group("https://example.com/"), None);
}

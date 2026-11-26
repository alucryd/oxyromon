use super::*;

#[test]
fn test() {
    let system = System {
        id: 1,
        name: "Nintendo - Nintendo 64DD".to_string(),
        custom_name: None,
        custom_extension: None,
        description: String::new(),
        version: String::new(),
        url: None,
        completion: 0,
        arcade: false,
        merging: 0,
        games_complete: 0,
        games_total: 0,
    };
    // its disks and its IPL BIOS, none of which a header tells
    assert_eq!(cartridge_platform(&system), Some("n64dd"));
}

use super::*;

#[test]
fn test() {
    let system = |name: &str| System {
        id: 1,
        name: name.to_string(),
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
    // the card DATs, decrypted or not, of both models
    for name in [
        "Nintendo - Nintendo 3DS (Decrypted)",
        "Nintendo - Nintendo 3DS (Encrypted)",
        "Nintendo - New Nintendo 3DS (Decrypted)",
    ] {
        assert_eq!(cartridge_platform(&system(name)), Some("3ds"), "{name}");
    }
    // the eShop's, CIAs' contents rather than cards
    assert_eq!(
        cartridge_platform(&system("Nintendo - Nintendo 3DS (Digital) (CDN)")),
        None
    );
}

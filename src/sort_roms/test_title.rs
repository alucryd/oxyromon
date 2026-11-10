use super::*;

#[test]
fn test() {
    // regions, languages, versions and release statuses are left out
    assert_eq!(get_title("Test Game (USA)"), "test game");
    assert_eq!(get_title("Test Game (Europe) (En,Fr,De)"), "test game");
    assert_eq!(get_title("Test Game (Japan) (Rev 1)"), "test game");
    assert_eq!(get_title("Test Game (USA) (v1.1)"), "test game");
    assert_eq!(get_title("Test Game (USA, Europe) (Beta)"), "test game");

    // and so are the flags that only tell editions apart
    assert_eq!(
        get_title("Test Game (Japan) (PlayStation the Best)"),
        "test game"
    );
    assert_eq!(get_title("Test Game (Japan) (Premium Box)"), "test game");
    assert_eq!(
        get_title("Test Game (Japan) (Shokai Genteiban)"),
        "test game"
    );
    assert_eq!(get_title("Test Game (Europe) (EDC)"), "test game");
    assert_eq!(get_title("Test Game (Europe) (Made in USA)"), "test game");
    assert_eq!(get_title("Test Game (Japan) (SLPS-01234)"), "test game");
    assert_eq!(get_title("Test Game (Japan) (2M, 3M)"), "test game");
    assert_eq!(get_title("Test Game (USA) (Trade Demo)"), "test game");

    // a disc number stays
    assert_eq!(
        get_title("Test Game (Europe) (Disc 2)"),
        "test game (disc 2)"
    );
    assert_eq!(
        get_title("Test Game (Japan) (Disc 2) (Premium Box)"),
        "test game (disc 2)"
    );

    // and so does any other flag, and one naming a disc of its own
    assert_eq!(
        get_title("Test Game (Japan) (Joukan)"),
        "test game (joukan)"
    );
    assert_eq!(
        get_title("Test Game (USA) (Bonus Disc)"),
        "test game (bonus disc)"
    );
    assert_eq!(
        get_title("Test Game (USA) (Premium Pack Bonus Disc)"),
        "test game (premium pack bonus disc)"
    );

    // a name outside No-Intro's convention is its own title
    assert_eq!(
        get_title("Test Game (1995)(Publisher)"),
        "Test Game (1995)(Publisher)"
    );
}

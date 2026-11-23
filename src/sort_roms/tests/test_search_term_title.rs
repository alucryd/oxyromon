use super::*;

#[test]
fn test() {
    // a search term reads as a name without its regions
    assert_eq!(
        get_search_term_title_and_editions("Test Game"),
        (String::from("test game"), vec![])
    );
    assert_eq!(
        get_search_term_title_and_editions("Test Game (Disc 1)"),
        (String::from("test game (disc 1)"), vec![])
    );

    // and the edition flags it names are left out of its title
    assert_eq!(
        get_search_term_title_and_editions("Test Game (Disc 1) (Premium Box)"),
        (
            String::from("test game (disc 1)"),
            vec![String::from("premium box")]
        )
    );

    // a title with brackets of its own keeps them, as in a game's name
    assert_eq!(
        get_search_term_title_and_editions("Test Game (The Movie) - Part 2").0,
        get_title_and_editions("Test Game (The Movie) - Part 2 (Europe)").0
    );

    // a game names the edition flags left out of its title
    assert_eq!(
        get_title_and_editions("Test Game (Japan) (Disc 1) (Premium Box)"),
        (
            String::from("test game (disc 1)"),
            vec![String::from("premium box")]
        )
    );
}

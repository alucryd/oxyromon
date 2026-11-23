use super::*;

#[test]
fn test() {
    assert_eq!(
        title_key("Uncharted: Drake's Fortune"),
        title_key("Uncharted - Drake's Fortune (USA) (En,Fr,Es)")
    );
    assert_eq!(title_key("GRAN TURISMO® 5"), "granturismo5");
    assert_ne!(
        title_key("Gran Turismo 5"),
        title_key("Gran Turismo 5 Prologue (USA)")
    );
    assert_ne!(title_key("Killzone 2"), title_key("Killzone 3 (USA) [b]"));
}

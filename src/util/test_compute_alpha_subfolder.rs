use super::*;

#[test]
fn test_uppercase_letter() {
    assert_eq!(compute_alpha_subfolder("Ace Combat"), "A");
}

#[test]
fn test_lowercase_letter() {
    assert_eq!(compute_alpha_subfolder("zero Wing"), "Z");
}

#[test]
fn test_digit() {
    assert_eq!(compute_alpha_subfolder("007 GoldenEye"), "#");
}

#[test]
fn test_special_character() {
    assert_eq!(compute_alpha_subfolder("!Special"), "#");
}

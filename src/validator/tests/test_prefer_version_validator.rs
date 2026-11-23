use async_graphql::CustomValidator;

use super::*;

#[test]
fn test_valid() {
    let validator = PreferVersionValidator::new();
    assert!(validator.check(&String::from("none")).is_ok());
    assert!(validator.check(&String::from("new")).is_ok());
    assert!(validator.check(&String::from("old")).is_ok());
}

#[test]
fn test_invalid() {
    let validator = PreferVersionValidator::new();
    assert!(validator.check(&String::from("invalid")).is_err());
    assert!(validator.check(&String::from("")).is_err());
}

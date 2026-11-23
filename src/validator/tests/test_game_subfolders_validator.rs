use async_graphql::CustomValidator;

use super::*;

#[test]
fn test_valid() {
    let validator = GameSubfoldersValidator::new();
    assert!(validator.check(&String::from("never")).is_ok());
    assert!(validator.check(&String::from("multiple")).is_ok());
    assert!(validator.check(&String::from("always")).is_ok());
}

#[test]
fn test_invalid() {
    let validator = GameSubfoldersValidator::new();
    assert!(validator.check(&String::from("true")).is_err());
    assert!(validator.check(&String::from("")).is_err());
}

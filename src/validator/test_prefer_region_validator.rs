use async_graphql::CustomValidator;

use super::*;

#[test]
fn test_valid() {
    let validator = PreferRegionValidator::new();
    assert!(validator.check(&String::from("none")).is_ok());
    assert!(validator.check(&String::from("broad")).is_ok());
    assert!(validator.check(&String::from("narrow")).is_ok());
}

#[test]
fn test_invalid() {
    let validator = PreferRegionValidator::new();
    assert!(validator.check(&String::from("invalid")).is_err());
    assert!(validator.check(&String::from("")).is_err());
}

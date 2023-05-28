use super::config::{PreferRegion, PreferVersion};
use async_graphql::{CustomValidator, InputValueError};
use strum::VariantNames;

pub struct PreferRegionValidator;

impl PreferRegionValidator {
    pub fn new() -> Self {
        PreferRegionValidator {}
    }
}

impl CustomValidator<String> for PreferRegionValidator {
    fn check(&self, value: &String) -> Result<(), InputValueError<String>> {
        if PreferRegion::VARIANTS.contains(&value.as_str()) {
            Ok(())
        } else {
            Err(InputValueError::custom(format!(
                "Valid choices: {:?}",
                PreferRegion::VARIANTS
            )))
        }
    }
}

pub struct PreferVersionValidator;

impl PreferVersionValidator {
    pub fn new() -> Self {
        PreferVersionValidator {}
    }
}

impl CustomValidator<String> for PreferVersionValidator {
    fn check(&self, value: &String) -> Result<(), InputValueError<String>> {
        if PreferVersion::VARIANTS.contains(&value.as_str()) {
            Ok(())
        } else {
            Err(InputValueError::custom(format!(
                "Valid choices: {:?}",
                PreferVersion::VARIANTS
            )))
        }
    }
}

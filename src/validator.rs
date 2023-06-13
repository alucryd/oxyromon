use super::config::{PreferredRegion, PreferredVersion, SubfolderScheme};
use async_graphql::{CustomValidator, InputValueError};
use std::path::Path;
use strum::VariantNames;

pub struct PreferRegionValidator;

impl PreferRegionValidator {
    pub fn new() -> Self {
        PreferRegionValidator {}
    }
}

impl CustomValidator<String> for PreferRegionValidator {
    fn check(&self, value: &String) -> Result<(), InputValueError<String>> {
        if PreferredRegion::VARIANTS.contains(&value.as_str()) {
            Ok(())
        } else {
            Err(InputValueError::custom(format!(
                "Valid choices: {:?}",
                PreferredRegion::VARIANTS
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
        if PreferredVersion::VARIANTS.contains(&value.as_str()) {
            Ok(())
        } else {
            Err(InputValueError::custom(format!(
                "Valid choices: {:?}",
                PreferredVersion::VARIANTS
            )))
        }
    }
}

pub struct SubfolderSchemeValidator;

impl SubfolderSchemeValidator {
    pub fn new() -> Self {
        SubfolderSchemeValidator {}
    }
}

impl CustomValidator<String> for SubfolderSchemeValidator {
    fn check(&self, value: &String) -> Result<(), InputValueError<String>> {
        if SubfolderScheme::VARIANTS.contains(&value.as_str()) {
            Ok(())
        } else {
            Err(InputValueError::custom(format!(
                "Valid choices: {:?}",
                SubfolderScheme::VARIANTS
            )))
        }
    }
}

pub struct DirectoryValidator;

impl DirectoryValidator {
    pub fn new() -> Self {
        DirectoryValidator {}
    }
}

impl CustomValidator<String> for DirectoryValidator {
    fn check(&self, value: &String) -> Result<(), InputValueError<String>> {
        if Path::new(&value).canonicalize()?.is_dir() {
            Ok(())
        } else {
            Err(InputValueError::custom("Missing or invalid directory"))
        }
    }
}

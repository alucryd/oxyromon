use super::config::{PreferredRegion, PreferredVersion, SubfolderScheme};
use anyhow::Result;
use async_graphql::{CustomValidator, InputValueError};
use std::marker::PhantomData;
use std::path::Path;
use strum::VariantNames;

pub struct VariantValidator<E>(PhantomData<E>);

impl<E: VariantNames + 'static> VariantValidator<E> {
    pub fn new() -> Self {
        VariantValidator(PhantomData)
    }
}

impl<E: VariantNames + 'static> CustomValidator<String> for VariantValidator<E> {
    fn check(&self, value: &String) -> Result<(), InputValueError<String>> {
        if E::VARIANTS.contains(&value.as_str()) {
            Ok(())
        } else {
            Err(InputValueError::custom(format!(
                "Valid choices: {:?}",
                E::VARIANTS
            )))
        }
    }
}

pub type PreferRegionValidator = VariantValidator<PreferredRegion>;
pub type PreferVersionValidator = VariantValidator<PreferredVersion>;
pub type SubfolderSchemeValidator = VariantValidator<SubfolderScheme>;

/// Validates a `PREFER_FORMAT` value against the convert formats.
pub struct PreferFormatValidator;

impl PreferFormatValidator {
    pub fn new() -> Self {
        PreferFormatValidator
    }
}

impl CustomValidator<String> for PreferFormatValidator {
    fn check(&self, value: &String) -> Result<(), InputValueError<String>> {
        if crate::convert_roms::ALL_FORMATS.contains(&value.as_str()) {
            Ok(())
        } else {
            Err(InputValueError::custom(format!(
                "Valid choices: {:?}",
                crate::convert_roms::ALL_FORMATS
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

#[cfg(test)]
mod test_directory_validator;
#[cfg(test)]
mod test_prefer_region_validator;
#[cfg(test)]
mod test_prefer_version_validator;
#[cfg(test)]
mod test_subfolder_scheme_validator;

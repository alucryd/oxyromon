use async_graphql::CustomValidator;
use tempfile::TempDir;

use super::*;

#[test]
fn test_valid() {
    let tmp_directory = TempDir::new().unwrap();
    let validator = DirectoryValidator::new();
    assert!(
        validator
            .check(&String::from(tmp_directory.path().to_str().unwrap()))
            .is_ok()
    );
}

#[test]
fn test_missing_directory() {
    let validator = DirectoryValidator::new();
    assert!(validator.check(&String::from("/nonexistent/path")).is_err());
}

#[test]
fn test_file_not_directory() {
    let tmp_file = tempfile::NamedTempFile::new().unwrap();
    let validator = DirectoryValidator::new();
    assert!(
        validator
            .check(&String::from(tmp_file.path().to_str().unwrap()))
            .is_err()
    );
}

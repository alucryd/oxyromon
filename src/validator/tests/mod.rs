//! Per-file tests for this module. The re-export keeps each
//! file's `use super::*;` pointing at the subcommand's namespace.
pub use super::*;

mod test_directory_validator;
mod test_game_subfolders_validator;
mod test_prefer_region_validator;
mod test_prefer_version_validator;
mod test_subfolder_scheme_validator;

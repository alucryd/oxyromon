//! Per-file tests for this module. The re-export keeps each
//! file's `use super::*;` pointing at the subcommand's namespace.
pub use super::*;

mod test_import_clone_lists;
mod test_import_clone_lists_release_group;
mod test_import_clone_lists_unknown_system;
mod test_split_release_group;

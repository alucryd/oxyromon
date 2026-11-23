//! Per-file tests for this module. The re-export keeps each
//! file's `use super::*;` pointing at the subcommand's namespace.
pub use super::*;

mod test_add_to_list;
mod test_add_to_list_already_exists;
mod test_bool;
mod test_chd_settings;
mod test_cli;
mod test_directory;
mod test_list;
mod test_remove_from_list;
mod test_remove_from_list_does_not_exist;
mod test_set_new_directory_when_old_is_missing;
mod test_system_bool_fallback;
mod test_system_bool_override;

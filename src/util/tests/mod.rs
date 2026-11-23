//! Per-file tests for this module. The re-export keeps each
//! file's `use super::*;` pointing at the subcommand's namespace.
pub use super::*;

mod test_compute_alpha_subfolder;
mod test_system_directory_custom_name;
mod test_system_directory_group_non_redump;
mod test_system_directory_group_subsystems;
mod test_system_directory_no_group_subsystems;
mod test_trash_path;

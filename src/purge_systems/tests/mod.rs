//! Per-file tests for this module. The re-export keeps each
//! file's `use super::*;` pointing at the subcommand's namespace.
pub use super::*;

mod test_purge_systems;
mod test_purge_systems_arcade;
mod test_purge_systems_by_name;

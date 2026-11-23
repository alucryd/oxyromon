//! Per-file tests for this module. The re-export keeps each
//! file's `use super::*;` pointing at the subcommand's namespace.
pub use super::*;

mod test_iso_complete;
mod test_iso_complete_game_subfolders;
mod test_iso_incomplete;

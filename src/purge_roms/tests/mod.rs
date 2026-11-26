//! Per-file tests for this module. The re-export keeps each
//! file's `use super::*;` pointing at the subcommand's namespace.
pub use super::*;

mod test_foreign;
mod test_foreign_store;
mod test_foreign_store_unreadable_wua;
mod test_foreign_subfolder;
mod test_missing;
mod test_missing_last_ore_of_a_store;
mod test_missing_subfolder;
mod test_orphans;
mod test_orphans_game_subfolders;
mod test_orphans_subfolder;
mod test_trashed;
mod test_trashed_rom_directory;

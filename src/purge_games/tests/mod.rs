//! Per-file tests for this module. The re-export keeps each
//! file's `use super::*;` pointing at the subcommand's namespace.
pub use super::*;

mod test_purge_games;
mod test_purge_games_clash;
mod test_purge_games_folder;
mod test_purge_games_missing;
mod test_purge_games_parent;
mod test_purge_games_playlist;
mod test_purge_games_shared;

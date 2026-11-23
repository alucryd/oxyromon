//! Per-file tests for this module. The re-export keeps each
//! file's `use super::*;` pointing at the subcommand's namespace.
pub use super::*;

mod test_already_rebuilt;
mod test_full_non_merged_to_non_merged;
mod test_merged_loose_to_non_merged;
mod test_merged_to_non_merged;
mod test_non_merged_to_merged;
mod test_non_merged_to_split;
mod test_nothing_to_do;
mod test_split_to_full_non_merged;
mod test_split_to_merged;
mod test_split_to_non_merged;
mod test_split_to_non_merged_missing;

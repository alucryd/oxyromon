//! Per-file tests for this module. The re-export keeps each
//! file's `use super::*;` pointing at the subcommand's namespace.
pub use super::*;

mod test_mutations;
mod test_queries;
mod test_server;
mod test_store_index;
mod test_uploads;

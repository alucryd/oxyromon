//! Per-file tests for this module. The re-export keeps each
//! file's `use super::*;` pointing at the subcommand's namespace.
pub use super::*;

mod test_as_nsp_as_nsz;
mod test_check;
mod test_check_mismatch;
mod test_hash_and_size;
mod test_to_nsp;
mod test_to_nsz;

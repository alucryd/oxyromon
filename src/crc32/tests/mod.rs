//! Per-file tests for this module. The re-export keeps each
//! file's `use super::*;` pointing at the subcommand's namespace.
pub use super::*;

mod test_empty_input;
mod test_known_value;
mod test_reset;
mod test_write_trait;

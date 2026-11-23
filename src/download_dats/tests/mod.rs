//! Per-file tests for this module. The re-export keeps each
//! file's `use super::*;` pointing at the subcommand's namespace.
pub use super::*;

mod test_nointro;
mod test_nointro_failures;
mod test_redump;
mod test_redump_failures;

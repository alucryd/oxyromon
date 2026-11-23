//! Per-file tests for this module. The re-export keeps each
//! file's `use super::*;` pointing at the subcommand's namespace.
pub use super::*;

mod test_ird;
mod test_ird_orphan_subfolder;
mod test_title;

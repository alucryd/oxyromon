//! Per-file tests for this module. The re-export keeps each
//! file's `use super::*;` pointing at the subcommand's namespace.
pub use super::*;

mod test_cartridge_platform_3ds;
mod test_cartridge_platform_n64dd;
mod test_store_savings_split;
mod test_store_settings_base_system;

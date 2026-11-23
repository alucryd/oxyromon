//! Per-file tests for this module. The re-export keeps each
//! file's `use super::*;` pointing at the subcommand's namespace.
pub use super::*;

mod test_bps;
mod test_bps_by_rom_id;
mod test_bps_ips;
mod test_ips;
mod test_xdelta;

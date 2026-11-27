//! Per-file tests for this module. The re-export keeps each
//! file's `use super::*;` pointing at the subcommand's namespace.
pub use super::*;

mod test_import_3ds;
mod test_import_3ds_bad_key;
mod test_import_dkey;
mod test_import_keys;
mod test_import_otp;
mod test_import_switch;
mod test_import_switch_tampered;

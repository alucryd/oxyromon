//! Per-file tests for this module. The re-export keeps each
//! file's `use super::*;` pointing at the subcommand's namespace.
pub use super::*;

mod test_bps_matcher;
mod test_chd_matcher;
mod test_cso_matcher;
mod test_ips_matcher;
mod test_ird_matcher;
mod test_rdsk_matcher;
mod test_riff_matcher;
mod test_rvz_matcher;
mod test_xdelta_matcher;
mod test_zso_matcher;

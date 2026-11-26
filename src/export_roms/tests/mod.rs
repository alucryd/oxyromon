//! Per-file tests for this module. The re-export keeps each
//! file's `use super::*;` pointing at the subcommand's namespace.
pub use super::*;

mod matrix;
mod test_amiga_disks_ore_to_original;
mod test_merged_to_original;
mod test_multiple_tracks_chd_to_cso_should_do_nothing;
mod test_multiple_tracks_chd_to_zso_should_do_nothing;
mod test_multiple_tracks_cue_bin_to_gdi;
mod test_multiple_tracks_cue_bin_to_iso;
mod test_nus_missing_h3_to_original;
mod test_nus_sevenzip_to_original;
mod test_nus_stored_ticket_to_original;
mod test_nus_to_original;
mod test_original_to_zip_multiple_roms;
mod test_original_to_zip_with_correct_name;
mod test_original_to_zip_with_incorrect_name;
mod test_sevenzip_multiple_tracks_cue_bin_to_chd;
mod test_sevenzip_multiple_tracks_cue_bin_to_iso;
mod test_single_track_cue_bin_to_iso;
mod test_thin_ore_to_original_and_ore;
mod test_wua_to_nus;
mod test_wua_to_wux;
mod test_wux_to_nus;
mod test_wux_to_wua;
mod test_wux_to_wux_should_copy;
mod test_wux_with_stored_ticket_update_to_wua;
mod test_wux_with_ticketless_update_to_wua;
mod test_wux_with_update_to_wua;

//! Per-file tests for this module. The re-export keeps each
//! file's `use super::*;` pointing at the subcommand's namespace.
pub use super::*;

mod test_path_archive_custom_extension;
mod test_path_archive_multiple_files;
mod test_path_archive_single_file;
mod test_path_chd_multiple_tracks;
mod test_path_chd_single_track;
mod test_path_cso;
mod test_path_custom_extension;
mod test_path_game_subfolders;
mod test_path_game_subfolders_arcade_archive;
mod test_path_game_subfolders_disc;
mod test_path_game_subfolders_multiple;
mod test_path_game_subfolders_system;
mod test_path_original;
mod test_path_playlist;
mod test_path_playlist_game_subfolders;
mod test_path_playlist_subfolder_alpha;
mod test_path_rvz;
mod test_path_subfolder_alpha_letter;
mod test_path_subfolder_alpha_other;

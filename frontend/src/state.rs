//! Global reactive state, ported from `src/store.js`.
//!
//! Svelte writable stores become Leptos `RwSignal`s. `AppState` is `Copy`
//! (every `RwSignal` is `Copy`), provided once at the app root via
//! `provide_context` and pulled back with `expect_context` in any component.

use leptos::prelude::*;

use crate::model::{Game, Notification, Rom, Romfile, System};

// Setting keys (mirror store.js).
pub const ONE_REGIONS_KEY: &str = "REGIONS_ONE";
pub const ALL_REGIONS_KEY: &str = "REGIONS_ALL";
pub const LANGUAGES_KEY: &str = "LANGUAGES";
pub const DISCARD_RELEASES_KEY: &str = "DISCARD_RELEASES";
pub const DISCARD_FLAGS_KEY: &str = "DISCARD_FLAGS";
pub const STRICT_ONE_REGIONS_KEY: &str = "REGIONS_ONE_STRICT";
pub const PREFER_PARENTS_KEY: &str = "PREFER_PARENTS";
pub const PREFER_REGIONS_KEY: &str = "PREFER_REGIONS";
pub const PREFER_VERSIONS_KEY: &str = "PREFER_VERSIONS";
pub const PREFER_FLAGS_KEY: &str = "PREFER_FLAGS";
pub const ROM_DIRECTORY_KEY: &str = "ROM_DIRECTORY";
pub const TMP_DIRECTORY_KEY: &str = "TMP_DIRECTORY";
pub const GROUP_SUBSYSTEMS_KEY: &str = "GROUP_SUBSYSTEMS";
pub const ONE_REGIONS_SUBFOLDERS_KEY: &str = "REGIONS_ONE_SUBFOLDERS";
pub const ALL_REGIONS_SUBFOLDERS_KEY: &str = "REGIONS_ALL_SUBFOLDERS";

pub const PREFER_REGIONS_CHOICES: [&str; 3] = ["none", "broad", "narrow"];
pub const PREFER_VERSIONS_CHOICES: [&str; 3] = ["none", "new", "old"];
pub const SUBFOLDER_SCHEMES_CHOICES: [&str; 2] = ["none", "alpha"];

pub const PAGE_SIZE: usize = 20;
pub const ROMS_PAGE_SIZE: usize = 8;

#[derive(Clone, Copy)]
pub struct AppState {
    // Selection.
    pub system_id: RwSignal<i64>,
    pub game_id: RwSignal<i64>,
    pub purging_system_id: RwSignal<i64>,

    // Raw (unfiltered) and paginated data.
    pub unfiltered_systems: RwSignal<Vec<System>>,
    pub systems: RwSignal<Vec<System>>,
    pub unfiltered_games: RwSignal<Vec<Game>>,
    pub games: RwSignal<Vec<Game>>,
    pub unfiltered_roms: RwSignal<Vec<Rom>>,
    pub roms: RwSignal<Vec<Rom>>,
    pub romfiles: RwSignal<Vec<Romfile>>,

    // Sizes.
    pub total_original_size: RwSignal<i64>,
    pub one_region_original_size: RwSignal<i64>,
    pub total_actual_size: RwSignal<i64>,
    pub one_region_actual_size: RwSignal<i64>,

    // Pagination (1-indexed pages).
    pub systems_page: RwSignal<usize>,
    pub systems_total_pages: RwSignal<usize>,
    pub games_page: RwSignal<usize>,
    pub games_total_pages: RwSignal<usize>,
    pub roms_page: RwSignal<usize>,
    pub roms_total_pages: RwSignal<usize>,
    pub romfiles_page: RwSignal<usize>,
    pub romfiles_total_pages: RwSignal<usize>,

    // Filters.
    pub complete_filter: RwSignal<bool>,
    pub incomplete_filter: RwSignal<bool>,
    pub wanted_filter: RwSignal<bool>,
    pub ignored_filter: RwSignal<bool>,
    pub one_region_filter: RwSignal<bool>,
    pub name_filter: RwSignal<String>,

    // Modals.
    pub import_dat_modal_open: RwSignal<bool>,
    pub settings_modal_open: RwSignal<bool>,
    pub about_modal_open: RwSignal<bool>,

    // Notifications + transient toast.
    pub notifications: RwSignal<Vec<Notification>>,
    pub toast: RwSignal<Option<Notification>>,

    // Loading flags.
    pub loading_systems: RwSignal<bool>,
    pub loading_games: RwSignal<bool>,
    pub loading_roms: RwSignal<bool>,
    pub loading_sizes: RwSignal<bool>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            system_id: RwSignal::new(-1),
            game_id: RwSignal::new(-1),
            purging_system_id: RwSignal::new(-1),

            unfiltered_systems: RwSignal::new(Vec::new()),
            systems: RwSignal::new(Vec::new()),
            unfiltered_games: RwSignal::new(Vec::new()),
            games: RwSignal::new(Vec::new()),
            unfiltered_roms: RwSignal::new(Vec::new()),
            roms: RwSignal::new(Vec::new()),
            romfiles: RwSignal::new(Vec::new()),

            total_original_size: RwSignal::new(0),
            one_region_original_size: RwSignal::new(0),
            total_actual_size: RwSignal::new(0),
            one_region_actual_size: RwSignal::new(0),

            systems_page: RwSignal::new(1),
            systems_total_pages: RwSignal::new(1),
            games_page: RwSignal::new(1),
            games_total_pages: RwSignal::new(1),
            roms_page: RwSignal::new(1),
            roms_total_pages: RwSignal::new(1),
            romfiles_page: RwSignal::new(1),
            romfiles_total_pages: RwSignal::new(1),

            complete_filter: RwSignal::new(true),
            incomplete_filter: RwSignal::new(true),
            wanted_filter: RwSignal::new(true),
            ignored_filter: RwSignal::new(true),
            one_region_filter: RwSignal::new(false),
            name_filter: RwSignal::new(String::new()),

            import_dat_modal_open: RwSignal::new(false),
            settings_modal_open: RwSignal::new(false),
            about_modal_open: RwSignal::new(false),

            notifications: RwSignal::new(Vec::new()),
            toast: RwSignal::new(None),

            loading_systems: RwSignal::new(false),
            loading_games: RwSignal::new(false),
            loading_roms: RwSignal::new(false),
            loading_sizes: RwSignal::new(false),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Split a `|`-separated list value into a `Vec<String>` (mirrors `splitList`).
pub fn split_list(value: &Option<String>) -> Vec<String> {
    match value {
        Some(v) if !v.is_empty() => v.split('|').map(|s| s.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// `pretty-bytes`-style human readable size (decimal units, like the JS lib).
pub fn format_bytes(bytes: i64) -> String {
    if bytes == 0 {
        return "0 B".to_string();
    }
    let negative = bytes < 0;
    let mut value = bytes.unsigned_abs() as f64;
    const UNITS: [&str; 9] = ["B", "kB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    let formatted = if unit == 0 {
        format!("{} {}", value as i64, UNITS[unit])
    } else {
        // Trim to at most 3 significant digits like pretty-bytes' default.
        let s = format!("{:.2}", value);
        let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        format!("{} {}", s, UNITS[unit])
    };
    if negative {
        format!("-{}", formatted)
    } else {
        formatted
    }
}

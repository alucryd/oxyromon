//! Global reactive state.
//!
//! `AppState` is `Copy` (signals and memos both are), provided once at the app
//! root via `provide_context` and pulled back with `expect_context` in any
//! component.
//!
//! Source state is held in `RwSignal`s; everything the views derive from it —
//! the filtered and paginated slices, the page counts — is a `Memo` that pulls
//! from those sources when read, rather than something an effect has to push
//! into place after every change.

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

/// Number of pages needed to show `len` items, at least one.
pub fn total_pages(len: usize, page_size: usize) -> usize {
    len.div_ceil(page_size).max(1)
}

/// Number of items to skip to reach the start of `page` (1-indexed).
fn offset(page: usize, page_size: usize) -> usize {
    page_size.saturating_mul(page.saturating_sub(1))
}

/// Clone out just the items on `page`.
fn paginate<T: Clone>(items: &[T], page: usize, page_size: usize) -> Vec<T> {
    items
        .iter()
        .skip(offset(page, page_size))
        .take(page_size)
        .cloned()
        .collect()
}

/// Snapshot of the game filters. Derived once and shared by the memos that
/// count and paginate the games, so the filter signals are read once per
/// change rather than once per game.
#[derive(Clone, PartialEq)]
struct GameFilter {
    complete: bool,
    incomplete: bool,
    wanted: bool,
    ignored: bool,
    one_region: bool,
    needle: String,
}

impl GameFilter {
    fn keep(&self, game: &Game) -> bool {
        if !self.complete && game.completion == 2 {
            return false;
        }
        if !self.incomplete && game.completion == 1 {
            return false;
        }
        if !self.wanted && game.completion == 0 {
            return false;
        }
        if !self.ignored && game.sorting == 2 {
            return false;
        }
        if self.one_region && game.sorting != 1 {
            return false;
        }
        if !self.needle.is_empty() && !game.name_lower.contains(&self.needle) {
            return false;
        }
        true
    }
}

/// Global reactive state.
///
/// Fields are split into *sources* — the things that are fetched or that the
/// user drives directly — and *derived* values, which are [`Memo`]s recomputed
/// from those sources on demand. Nothing writes a derived field; adding a new
/// view of the data means adding a memo, not another effect that has to
/// remember to fire.
#[derive(Clone, Copy)]
pub struct AppState {
    // === Sources ===

    // Selection.
    pub system_id: RwSignal<i64>,
    pub game_id: RwSignal<i64>,
    pub purging_system_id: RwSignal<i64>,

    // Fetched data.
    pub unfiltered_systems: RwSignal<Vec<System>>,
    pub unfiltered_games: RwSignal<Vec<Game>>,
    pub unfiltered_roms: RwSignal<Vec<Rom>>,

    // Sizes.
    pub total_original_size: RwSignal<i64>,
    pub one_region_original_size: RwSignal<i64>,
    pub total_actual_size: RwSignal<i64>,
    pub one_region_actual_size: RwSignal<i64>,

    // Current page (1-indexed).
    pub systems_page: RwSignal<usize>,
    pub games_page: RwSignal<usize>,
    pub roms_page: RwSignal<usize>,
    pub romfiles_page: RwSignal<usize>,

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

    // === Derived ===
    /// The page of each list that is currently on screen.
    pub systems: Memo<Vec<System>>,
    pub games: Memo<Vec<Game>>,
    pub roms: Memo<Vec<Rom>>,
    pub romfiles: Memo<Vec<Romfile>>,

    /// Every distinct romfile of the selected game, deduplicated by path.
    pub unique_romfiles: Memo<Vec<Romfile>>,

    pub systems_total_pages: Memo<usize>,
    pub games_total_pages: Memo<usize>,
    pub roms_total_pages: Memo<usize>,
    pub romfiles_total_pages: Memo<usize>,
}

impl AppState {
    /// Must be called inside a reactive owner (i.e. from a component), since it
    /// creates the derived memos.
    pub fn new() -> Self {
        // --- Sources ---
        let system_id = RwSignal::new(-1);
        let game_id = RwSignal::new(-1);
        let purging_system_id = RwSignal::new(-1);

        let unfiltered_systems = RwSignal::new(Vec::<System>::new());
        let unfiltered_games = RwSignal::new(Vec::<Game>::new());
        let unfiltered_roms = RwSignal::new(Vec::<Rom>::new());

        let systems_page = RwSignal::new(1);
        let games_page = RwSignal::new(1);
        let roms_page = RwSignal::new(1);
        let romfiles_page = RwSignal::new(1);

        let complete_filter = RwSignal::new(true);
        let incomplete_filter = RwSignal::new(true);
        let wanted_filter = RwSignal::new(true);
        let ignored_filter = RwSignal::new(true);
        let one_region_filter = RwSignal::new(false);
        let name_filter = RwSignal::new(String::new());

        // --- Derived ---
        let systems_total_pages =
            Memo::new(move |_| total_pages(unfiltered_systems.with(Vec::len), PAGE_SIZE));
        let systems = Memo::new(move |_| {
            unfiltered_systems.with(|systems| paginate(systems, systems_page.get(), PAGE_SIZE))
        });

        let game_filter = Memo::new(move |_| GameFilter {
            complete: complete_filter.get(),
            incomplete: incomplete_filter.get(),
            wanted: wanted_filter.get(),
            ignored: ignored_filter.get(),
            one_region: one_region_filter.get(),
            needle: name_filter.get().to_lowercase(),
        });
        // Only feeds the page count; kept separate so counting does not have to
        // clone or allocate the matching games.
        let filtered_game_count = Memo::new(move |_| {
            game_filter.with(|filter| {
                unfiltered_games.with(|games| games.iter().filter(|game| filter.keep(game)).count())
            })
        });
        let games_total_pages =
            Memo::new(move |_| total_pages(filtered_game_count.get(), PAGE_SIZE));
        // Filters and paginates in one pass, so only the visible rows are cloned
        // no matter how large the system is.
        let games = Memo::new(move |_| {
            let skip = offset(games_page.get(), PAGE_SIZE);
            game_filter.with(|filter| {
                unfiltered_games.with(|games| {
                    games
                        .iter()
                        .filter(|game| filter.keep(game))
                        .skip(skip)
                        .take(PAGE_SIZE)
                        .cloned()
                        .collect()
                })
            })
        });

        let roms_total_pages =
            Memo::new(move |_| total_pages(unfiltered_roms.with(Vec::len), ROMS_PAGE_SIZE));
        let roms = Memo::new(move |_| {
            unfiltered_roms.with(|roms| paginate(roms, roms_page.get(), ROMS_PAGE_SIZE))
        });

        let unique_romfiles = Memo::new(move |_| {
            unfiltered_roms.with(|roms| {
                let mut romfiles: Vec<Romfile> =
                    roms.iter().filter_map(|rom| rom.romfile.clone()).collect();
                romfiles.sort_by(|a, b| a.path.cmp(&b.path));
                romfiles.dedup_by(|a, b| a.path == b.path);
                romfiles
            })
        });
        let romfiles_total_pages =
            Memo::new(move |_| total_pages(unique_romfiles.with(Vec::len), ROMS_PAGE_SIZE));
        let romfiles = Memo::new(move |_| {
            unique_romfiles.with(|romfiles| paginate(romfiles, romfiles_page.get(), ROMS_PAGE_SIZE))
        });

        Self {
            system_id,
            game_id,
            purging_system_id,

            unfiltered_systems,
            unfiltered_games,
            unfiltered_roms,

            total_original_size: RwSignal::new(0),
            one_region_original_size: RwSignal::new(0),
            total_actual_size: RwSignal::new(0),
            one_region_actual_size: RwSignal::new(0),

            systems_page,
            games_page,
            roms_page,
            romfiles_page,

            complete_filter,
            incomplete_filter,
            wanted_filter,
            ignored_filter,
            one_region_filter,
            name_filter,

            import_dat_modal_open: RwSignal::new(false),
            settings_modal_open: RwSignal::new(false),
            about_modal_open: RwSignal::new(false),

            notifications: RwSignal::new(Vec::new()),
            toast: RwSignal::new(None),

            loading_systems: RwSignal::new(false),
            loading_games: RwSignal::new(false),
            loading_roms: RwSignal::new(false),
            loading_sizes: RwSignal::new(false),

            systems,
            games,
            roms,
            romfiles,
            unique_romfiles,
            systems_total_pages,
            games_total_pages,
            roms_total_pages,
            romfiles_total_pages,
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

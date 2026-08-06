//! Global reactive state.
//!
//! `AppState` is `Copy` (signals and memos both are), provided once at the app
//! root via `provide_context` and pulled back with `expect_context` in any
//! component.
//!
//! Source state is held in `RwSignal`s; everything the views derive from it —
//! the filtered lists, the counts — is a `Memo` that pulls from those sources
//! when read, rather than something an effect has to push into place after
//! every change.

use leptos::prelude::*;

use std::ops::ControlFlow;

use leptos::task::spawn_local;

use crate::api::{fetch_roms, fetch_sizes, fetch_systems, stream_games};
use crate::model::{Game, Rom, Romfile, Sizes, System};
use crate::notify::Notifier;

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

/// Height of one row, in pixels. The games list is virtualized, so this has to
/// match the CSS (`--row-height` in `styles.css`) exactly.
pub const ROW_HEIGHT: f64 = 40.0;

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
    pub notifier: Notifier,

    // === Async data ===
    //
    // The games, roms and sizes resources re-run whenever the selection they
    // read changes, so nothing has to remember to kick off a fetch; they are
    // reached only through the memos below. The systems resource reads no
    // signal, so it is kept here to be refetched explicitly when an SSE event
    // says the set of systems changed.
    pub systems_resource: LocalResource<Vec<System>>,

    /// Games of the selected system, growing as chunks arrive. Not a resource:
    /// a resource resolves to one value, and the point here is to be usable
    /// before the last chunk lands. Exposed so the list can clone just the rows
    /// it is about to draw.
    pub games: RwSignal<Vec<Game>>,

    // === Derived ===
    /// Lists rendered in full; these are bounded by one system or one game.
    pub systems: Memo<Vec<System>>,
    pub roms: Memo<Vec<Rom>>,

    /// Every distinct romfile of the selected game, deduplicated by path.
    pub romfiles: Memo<Vec<Romfile>>,

    /// Indices into `games_resource` of the games matching the filters, in
    /// order. A system can hold tens of thousands of games, so the list is
    /// kept as indices and the visible window is cloned out at draw time.
    pub filtered_games: Memo<Vec<usize>>,

    /// Totals for the statistics card (unfiltered).
    pub system_count: Memo<usize>,
    pub game_count: Memo<usize>,
    pub rom_count: Memo<usize>,

    pub sizes: Memo<Sizes>,

    /// Set for the duration of each fetch, by the resource wrappers above.
    pub loading_systems: RwSignal<bool>,
    pub loading_games: RwSignal<bool>,
    pub loading_roms: RwSignal<bool>,
    pub loading_sizes: RwSignal<bool>,
}

impl AppState {
    /// Must be called inside a reactive owner (i.e. from a component), since it
    /// creates the resources and derived memos.
    pub fn new() -> Self {
        // --- Sources ---
        let system_id = RwSignal::new(-1);
        let game_id = RwSignal::new(-1);
        let purging_system_id = RwSignal::new(-1);

        let complete_filter = RwSignal::new(true);
        let incomplete_filter = RwSignal::new(true);
        let wanted_filter = RwSignal::new(true);
        let ignored_filter = RwSignal::new(true);
        let one_region_filter = RwSignal::new(false);
        let name_filter = RwSignal::new(String::new());

        // Built before the resources, which report their failures through it.
        let notifier = Notifier::new();

        // --- Async data ---
        //
        // Leptos 0.8 exposes no public `loading()` on a resource, and keeps the
        // previous value while a refetch is in flight, so each fetch drives its
        // own flag. The memos below read it to blank a list while it reloads
        // rather than leave the previous selection's rows on screen.
        let loading_systems = RwSignal::new(true);
        let systems_resource = LocalResource::new(move || {
            loading_systems.set(true);
            async move {
                let systems = fetch_systems(notifier).await;
                loading_systems.set(false);
                systems
            }
        });

        // Games stream in rather than arriving whole; see the field comment.
        // Each run takes a generation, so a stream whose system has since been
        // deselected can tell that it should stop appending.
        let loading_games = RwSignal::new(true);
        let games = RwSignal::new(Vec::<Game>::new());
        let games_generation = RwSignal::new(0u64);
        Effect::new(move |_| {
            let system_id = system_id.get();
            let generation = games_generation.get_untracked().wrapping_add(1);
            games_generation.set(generation);
            games.set(Vec::new());
            loading_games.set(true);
            spawn_local(async move {
                stream_games(notifier, system_id, move |chunk| {
                    if games_generation.get_untracked() != generation {
                        return ControlFlow::Break(());
                    }
                    games.update(|games| games.extend(chunk));
                    ControlFlow::Continue(())
                })
                .await;
                if games_generation.get_untracked() == generation {
                    loading_games.set(false);
                }
            });
        });

        let loading_roms = RwSignal::new(true);
        let roms_resource = LocalResource::new(move || {
            // Only the game selection drives this. Changing system resets the
            // game to the sentinel, which re-runs this anyway; tracking the
            // system as well would first fire a doomed fetch pairing the new
            // system with the game selected under the old one.
            let game_id = game_id.get();
            let system_id = system_id.get_untracked();
            loading_roms.set(true);
            async move {
                let roms = fetch_roms(notifier, game_id, system_id).await;
                loading_roms.set(false);
                roms
            }
        });

        let loading_sizes = RwSignal::new(true);
        let sizes_resource = LocalResource::new(move || {
            let system_id = system_id.get();
            loading_sizes.set(true);
            async move {
                let sizes = fetch_sizes(notifier, system_id).await;
                loading_sizes.set(false);
                sizes
            }
        });

        let sizes = Memo::new(move |_| sizes_resource.map(|sizes| *sizes).unwrap_or_default());

        // --- Derived ---
        let system_count = Memo::new(move |_| systems_resource.map(Vec::len).unwrap_or(0));
        let game_count = Memo::new(move |_| games.with(Vec::len));
        let rom_count = Memo::new(move |_| roms_resource.map(Vec::len).unwrap_or(0));

        let systems = Memo::new(move |_| {
            systems_resource
                .map(|systems| systems.to_vec())
                .unwrap_or_default()
        });

        let game_filter = Memo::new(move |_| GameFilter {
            complete: complete_filter.get(),
            incomplete: incomplete_filter.get(),
            wanted: wanted_filter.get(),
            ignored: ignored_filter.get(),
            one_region: one_region_filter.get(),
            needle: name_filter.get().to_lowercase(),
        });
        // Indices rather than games: this recomputes on every keystroke in the
        // name filter, and a full arcade set is tens of thousands of entries.
        // No blanking while loading: the whole point of streaming is that the
        // rows already in hand stay on screen while the rest arrive.
        let filtered_games = Memo::new(move |_| {
            game_filter.with(|filter| {
                games.with(|games| {
                    games
                        .iter()
                        .enumerate()
                        .filter(|(_, game)| filter.keep(game))
                        .map(|(index, _)| index)
                        .collect()
                })
            })
        });

        let roms = Memo::new(move |_| {
            if loading_roms.get() {
                return Vec::new();
            }
            roms_resource.map(|roms| roms.to_vec()).unwrap_or_default()
        });

        let romfiles = Memo::new(move |_| {
            if loading_roms.get() {
                return Vec::new();
            }
            roms_resource
                .map(|roms| {
                    let mut romfiles: Vec<Romfile> =
                        roms.iter().filter_map(|rom| rom.romfile.clone()).collect();
                    romfiles.sort_by(|a, b| a.path.cmp(&b.path));
                    romfiles.dedup_by(|a, b| a.path == b.path);
                    romfiles
                })
                .unwrap_or_default()
        });

        Self {
            system_id,
            game_id,
            purging_system_id,

            complete_filter,
            incomplete_filter,
            wanted_filter,
            ignored_filter,
            one_region_filter,
            name_filter,

            import_dat_modal_open: RwSignal::new(false),
            settings_modal_open: RwSignal::new(false),
            about_modal_open: RwSignal::new(false),

            notifier,

            systems_resource,
            games,

            systems,
            roms,
            romfiles,
            filtered_games,

            system_count,
            game_count,
            rom_count,
            sizes,

            loading_systems,
            loading_games,
            loading_roms,
            loading_sizes,
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

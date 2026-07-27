//! GraphQL client and typed query/mutation helpers.
//!
//! Ports `src/query.js` and `src/mutation.js`. Queries POST to the same-origin
//! `/graphql` endpoint served by the Axum backend. Data-loading helpers take
//! `AppState` and imperatively update its signals, mirroring the store-updating
//! style of the original code.

use gloo_net::http::Request;
use leptos::prelude::*;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::model::{Game, Info, NotificationKind, Rom, Romfile, Setting, System};
use crate::notify::push_notification;
use crate::state::{AppState, PAGE_SIZE, ROMS_PAGE_SIZE};

/// POST a GraphQL document and return the `data` object, or an error string.
async fn graphql(query: &str, variables: Value) -> Result<Value, String> {
    let body = json!({ "query": query, "variables": variables });
    let response = Request::post("/graphql")
        .json(&body)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let mut value: Value = response.json().await.map_err(|e| e.to_string())?;
    if let Some(errors) = value.get("errors") {
        // Surface the first message rather than the raw errors array, which is
        // what ends up in front of the user.
        let message = errors
            .as_array()
            .and_then(|errors| errors.first())
            .and_then(|error| error["message"].as_str())
            .unwrap_or("unknown GraphQL error");
        return Err(message.to_string());
    }
    Ok(value["data"].take())
}

/// Log a failed request and tell the user about it.
///
/// Without this the UI silently renders empty tables when the backend is
/// unreachable or a query fails.
pub fn report_error(state: AppState, action: &str, error: &str) {
    leptos::logging::error!("{action}: {error}");
    push_notification(
        state,
        format!("{action} failed: {error}"),
        NotificationKind::Error,
    );
}

/// Deserialize a named field of the GraphQL `data` object.
fn field<T: DeserializeOwned>(data: &mut Value, name: &str) -> Result<T, String> {
    serde_json::from_value(data[name].take()).map_err(|e| e.to_string())
}

/// Half-open range of `items` covered by `page` (1-indexed), clamped to `len`.
fn page_range(len: usize, page: usize, page_size: usize) -> std::ops::Range<usize> {
    let start = page_size.saturating_mul(page.saturating_sub(1));
    if start >= len {
        return 0..0;
    }
    start..(page_size.saturating_mul(page)).min(len)
}

/// Clone out just the items on `page`, leaving the rest borrowed.
fn paginate<T: Clone>(items: &[T], page: usize, page_size: usize) -> Vec<T> {
    items[page_range(items.len(), page, page_size)].to_vec()
}

/// As [`paginate`], for an already-filtered list of borrows.
fn paginate_refs<T: Clone>(items: &[&T], page: usize, page_size: usize) -> Vec<T> {
    items[page_range(items.len(), page, page_size)]
        .iter()
        .map(|item| (*item).clone())
        .collect()
}

fn total_pages(len: usize, page_size: usize) -> usize {
    len.div_ceil(page_size).max(1)
}

// === Queries ===

pub async fn get_info() -> Result<Info, String> {
    let query = r#"{
        version
        dependencies { name version }
        systemCount
        gameCount
        romCount
    }"#;
    let data = graphql(query, Value::Null).await?;
    serde_json::from_value(data).map_err(|e| e.to_string())
}

pub async fn get_raw_settings(system_id: Option<i64>) -> Result<Vec<Setting>, String> {
    match system_id {
        Some(id) => {
            let query = format!("{{ systemSettings(systemId: {}) {{ key value }} }}", id);
            let mut data = graphql(&query, Value::Null).await?;
            field(&mut data, "systemSettings")
        }
        None => {
            let query = "{ settings { key value } }";
            let mut data = graphql(query, Value::Null).await?;
            field(&mut data, "settings")
        }
    }
}

pub async fn get_systems(state: AppState) {
    state.loading_systems.set(true);
    let query = r#"{
        systems { id name description completion merging arcade }
    }"#;
    match graphql(query, Value::Null).await {
        Ok(mut data) => match field::<Vec<System>>(&mut data, "systems") {
            Ok(systems) => {
                state.unfiltered_systems.set(systems);
                update_systems(state);
            }
            Err(e) => report_error(state, "Loading systems", &e),
        },
        Err(e) => report_error(state, "Loading systems", &e),
    }
    state.loading_systems.set(false);
}

pub fn update_systems(state: AppState) {
    let page = state.systems_page.get_untracked();
    let (total, current_page) = state.unfiltered_systems.with_untracked(|systems| {
        (
            total_pages(systems.len(), PAGE_SIZE),
            paginate(systems, page, PAGE_SIZE),
        )
    });
    state.systems_total_pages.set(total);
    state.systems.set(current_page);
}

pub async fn get_games_by_system_id(state: AppState, system_id: i64) {
    state.loading_games.set(true);
    let query = format!(
        "{{ games(systemId: {}) {{ id name description completion sorting }} }}",
        system_id
    );
    match graphql(&query, Value::Null).await {
        Ok(mut data) => match field::<Vec<Game>>(&mut data, "games") {
            Ok(mut games) => {
                for game in &mut games {
                    game.name_lower = game.name.to_lowercase();
                }
                state.unfiltered_games.set(games);
                update_games(state);
            }
            Err(e) => report_error(state, "Loading games", &e),
        },
        Err(e) => report_error(state, "Loading games", &e),
    }
    state.loading_games.set(false);
}

/// Snapshot of the game filters, read once per update rather than once per
/// game (these are signal reads, and the games list can hold tens of thousands
/// of entries for arcade systems).
struct GameFilter {
    complete: bool,
    incomplete: bool,
    wanted: bool,
    ignored: bool,
    one_region: bool,
    needle: String,
}

impl GameFilter {
    fn read(state: AppState) -> Self {
        Self {
            complete: state.complete_filter.get_untracked(),
            incomplete: state.incomplete_filter.get_untracked(),
            wanted: state.wanted_filter.get_untracked(),
            ignored: state.ignored_filter.get_untracked(),
            one_region: state.one_region_filter.get_untracked(),
            needle: state.name_filter.get_untracked().to_lowercase(),
        }
    }

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

pub fn update_games(state: AppState) {
    let filter = GameFilter::read(state);
    let page = state.games_page.get_untracked();
    // Borrow the full list and collect only references, so the single page we
    // hand to the view is the only thing actually cloned.
    let (total, current_page) = state.unfiltered_games.with_untracked(|games| {
        let matching: Vec<&Game> = games.iter().filter(|game| filter.keep(game)).collect();
        (
            total_pages(matching.len(), PAGE_SIZE),
            paginate_refs(&matching, page, PAGE_SIZE),
        )
    });
    state.games_total_pages.set(total);
    state.games.set(current_page);
}

pub async fn get_roms_by_game_and_system(state: AppState, game_id: i64, system_id: i64) {
    state.loading_roms.set(true);
    let query = format!(
        "{{ roms(gameId: {}) {{ id name size romfile {{ id path size }} ignored(systemId: {}) }} }}",
        game_id, system_id
    );
    match graphql(&query, Value::Null).await {
        Ok(mut data) => match field::<Vec<Rom>>(&mut data, "roms") {
            Ok(roms) => {
                state.unfiltered_roms.set(roms);
                update_roms(state);
            }
            Err(e) => report_error(state, "Loading ROMs", &e),
        },
        Err(e) => report_error(state, "Loading ROMs", &e),
    }
    state.loading_roms.set(false);
}

pub fn update_roms(state: AppState) {
    let page = state.roms_page.get_untracked();
    let (total, current_page) = state.unfiltered_roms.with_untracked(|roms| {
        (
            total_pages(roms.len(), ROMS_PAGE_SIZE),
            paginate(roms, page, ROMS_PAGE_SIZE),
        )
    });
    state.roms_total_pages.set(total);
    state.roms.set(current_page);
    update_romfiles(state);
}

pub fn update_romfiles(state: AppState) {
    let page = state.romfiles_page.get_untracked();
    let (total, current_page) = state.unfiltered_roms.with_untracked(|roms| {
        // Unique romfiles by path, sorted by path (mirrors uniqBy + sort).
        let mut romfiles: Vec<&Romfile> =
            roms.iter().filter_map(|rom| rom.romfile.as_ref()).collect();
        romfiles.sort_by(|a, b| a.path.cmp(&b.path));
        romfiles.dedup_by(|a, b| a.path == b.path);
        (
            total_pages(romfiles.len(), ROMS_PAGE_SIZE),
            paginate_refs(&romfiles, page, ROMS_PAGE_SIZE),
        )
    });
    state.romfiles_total_pages.set(total);
    state.romfiles.set(current_page);
}

pub async fn get_sizes_by_system_id(state: AppState, system_id: i64) {
    state.loading_sizes.set(true);
    let query = format!(
        "{{ totalOriginalSize(systemId: {id}) oneRegionOriginalSize(systemId: {id}) totalActualSize(systemId: {id}) oneRegionActualSize(systemId: {id}) }}",
        id = system_id
    );
    match graphql(&query, Value::Null).await {
        Ok(data) => {
            state
                .total_original_size
                .set(data["totalOriginalSize"].as_i64().unwrap_or(0));
            state
                .one_region_original_size
                .set(data["oneRegionOriginalSize"].as_i64().unwrap_or(0));
            state
                .total_actual_size
                .set(data["totalActualSize"].as_i64().unwrap_or(0));
            state
                .one_region_actual_size
                .set(data["oneRegionActualSize"].as_i64().unwrap_or(0));
        }
        Err(e) => report_error(state, "Loading statistics", &e),
    }
    state.loading_sizes.set(false);
}

// === Mutations ===

fn system_id_var(system_id: Option<i64>) -> Value {
    match system_id {
        Some(id) => json!(id),
        None => Value::Null,
    }
}

pub async fn add_to_list(key: &str, value: &str, system_id: Option<i64>) -> Result<(), String> {
    let mutation = r#"mutation AddToList($key: String!, $value: String!, $systemId: Int) {
        addToList(key: $key, value: $value, systemId: $systemId)
    }"#;
    graphql(
        mutation,
        json!({ "key": key, "value": value, "systemId": system_id_var(system_id) }),
    )
    .await
    .map(|_| ())
}

pub async fn remove_from_list(
    key: &str,
    value: &str,
    system_id: Option<i64>,
) -> Result<(), String> {
    let mutation = r#"mutation RemoveFromList($key: String!, $value: String!, $systemId: Int) {
        removeFromList(key: $key, value: $value, systemId: $systemId)
    }"#;
    graphql(
        mutation,
        json!({ "key": key, "value": value, "systemId": system_id_var(system_id) }),
    )
    .await
    .map(|_| ())
}

pub async fn set_bool(key: &str, value: bool, system_id: Option<i64>) -> Result<(), String> {
    let mutation = r#"mutation SetBool($key: String!, $value: Boolean!, $systemId: Int) {
        setBool(key: $key, value: $value, systemId: $systemId)
    }"#;
    graphql(
        mutation,
        json!({ "key": key, "value": value, "systemId": system_id_var(system_id) }),
    )
    .await
    .map(|_| ())
}

pub async fn set_prefer_regions(value: &str, system_id: Option<i64>) -> Result<(), String> {
    let mutation = r#"mutation SetPreferRegions($value: String!, $systemId: Int) {
        setPreferRegions(value: $value, systemId: $systemId)
    }"#;
    graphql(
        mutation,
        json!({ "value": value, "systemId": system_id_var(system_id) }),
    )
    .await
    .map(|_| ())
}

pub async fn set_prefer_versions(value: &str, system_id: Option<i64>) -> Result<(), String> {
    let mutation = r#"mutation SetPreferVersions($value: String!, $systemId: Int) {
        setPreferVersions(value: $value, systemId: $systemId)
    }"#;
    graphql(
        mutation,
        json!({ "value": value, "systemId": system_id_var(system_id) }),
    )
    .await
    .map(|_| ())
}

pub async fn set_subfolder_scheme(
    key: &str,
    value: &str,
    system_id: Option<i64>,
) -> Result<(), String> {
    let mutation = r#"mutation SetSubfolderScheme($key: String!, $value: String!, $systemId: Int) {
        setSubfolderScheme(key: $key, value: $value, systemId: $systemId)
    }"#;
    graphql(
        mutation,
        json!({ "key": key, "value": value, "systemId": system_id_var(system_id) }),
    )
    .await
    .map(|_| ())
}

pub async fn set_directory(key: &str, value: &str, system_id: Option<i64>) -> Result<(), String> {
    let mutation = r#"mutation SetDirectory($key: String!, $value: String!, $systemId: Int) {
        setDirectory(key: $key, value: $value, systemId: $systemId)
    }"#;
    graphql(
        mutation,
        json!({ "key": key, "value": value, "systemId": system_id_var(system_id) }),
    )
    .await
    .map(|_| ())
}

pub async fn purge_system(state: AppState, system_id: i64) {
    state.purging_system_id.set(system_id);
    let mutation = r#"mutation PurgeSystem($systemId: Int!) {
        purgeSystem(systemId: $systemId)
    }"#;
    if let Err(e) = graphql(mutation, json!({ "systemId": system_id })).await {
        report_error(state, "Purging the system", &e);
    }
    state.purging_system_id.set(-1);
}

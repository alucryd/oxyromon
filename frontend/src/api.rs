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

use crate::model::{Game, Info, Rom, Romfile, Setting, System};
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
        return Err(errors.to_string());
    }
    Ok(value["data"].take())
}

/// Deserialize a named field of the GraphQL `data` object.
fn field<T: DeserializeOwned>(data: &mut Value, name: &str) -> Result<T, String> {
    serde_json::from_value(data[name].take()).map_err(|e| e.to_string())
}

fn paginate<T: Clone>(items: &[T], page: usize, page_size: usize) -> Vec<T> {
    let start = page_size.saturating_mul(page.saturating_sub(1));
    if start >= items.len() {
        return Vec::new();
    }
    let end = (page_size.saturating_mul(page)).min(items.len());
    items[start..end].to_vec()
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
            let query = format!(
                "{{ systemSettings(systemId: {}) {{ key value }} }}",
                id
            );
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
            Err(e) => leptos::logging::error!("get_systems: {e}"),
        },
        Err(e) => leptos::logging::error!("get_systems: {e}"),
    }
    state.loading_systems.set(false);
}

pub fn update_systems(state: AppState) {
    let all = state.unfiltered_systems.get_untracked();
    state.systems_total_pages.set(total_pages(all.len(), PAGE_SIZE));
    let page = state.systems_page.get_untracked();
    state.systems.set(paginate(&all, page, PAGE_SIZE));
}

pub async fn get_games_by_system_id(state: AppState, system_id: i64) {
    state.loading_games.set(true);
    let query = format!(
        "{{ games(systemId: {}) {{ id name description completion sorting }} }}",
        system_id
    );
    match graphql(&query, Value::Null).await {
        Ok(mut data) => match field::<Vec<Game>>(&mut data, "games") {
            Ok(games) => {
                state.unfiltered_games.set(games);
                update_games(state);
            }
            Err(e) => leptos::logging::error!("get_games: {e}"),
        },
        Err(e) => leptos::logging::error!("get_games: {e}"),
    }
    state.loading_games.set(false);
}

fn filter_games(state: AppState, games: Vec<Game>) -> Vec<Game> {
    let name_filter = state.name_filter.get_untracked();
    let name_needle = name_filter.to_lowercase();
    games
        .into_iter()
        .filter(|game| {
            if !state.complete_filter.get_untracked() && game.completion == 2 {
                return false;
            }
            if !state.incomplete_filter.get_untracked() && game.completion == 1 {
                return false;
            }
            if !state.wanted_filter.get_untracked() && game.completion == 0 {
                return false;
            }
            if !state.ignored_filter.get_untracked() && game.sorting == 2 {
                return false;
            }
            if state.one_region_filter.get_untracked() && game.sorting != 1 {
                return false;
            }
            if !name_needle.is_empty() && !game.name.to_lowercase().contains(&name_needle) {
                return false;
            }
            true
        })
        .collect()
}

pub fn update_games(state: AppState) {
    let filtered = filter_games(state, state.unfiltered_games.get_untracked());
    state.games_total_pages.set(total_pages(filtered.len(), PAGE_SIZE));
    let page = state.games_page.get_untracked();
    state.games.set(paginate(&filtered, page, PAGE_SIZE));
    state.filtered_games.set(filtered);
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
            Err(e) => leptos::logging::error!("get_roms: {e}"),
        },
        Err(e) => leptos::logging::error!("get_roms: {e}"),
    }
    state.loading_roms.set(false);
}

pub fn update_roms(state: AppState) {
    let all = state.unfiltered_roms.get_untracked();
    state
        .roms_total_pages
        .set(total_pages(all.len(), ROMS_PAGE_SIZE));
    let page = state.roms_page.get_untracked();
    state.roms.set(paginate(&all, page, ROMS_PAGE_SIZE));
    update_romfiles(state);
}

pub fn update_romfiles(state: AppState) {
    // Unique romfiles by path, sorted by path (mirrors uniqBy + sort).
    let mut romfiles: Vec<Romfile> = state
        .unfiltered_roms
        .get_untracked()
        .into_iter()
        .filter_map(|rom| rom.romfile)
        .collect();
    romfiles.sort_by(|a, b| a.path.cmp(&b.path));
    romfiles.dedup_by(|a, b| a.path == b.path);

    state
        .romfiles_total_pages
        .set(total_pages(romfiles.len(), ROMS_PAGE_SIZE));
    let page = state.romfiles_page.get_untracked();
    state.romfiles.set(paginate(&romfiles, page, ROMS_PAGE_SIZE));
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
        Err(e) => leptos::logging::error!("get_sizes: {e}"),
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

pub async fn remove_from_list(key: &str, value: &str, system_id: Option<i64>) -> Result<(), String> {
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
        leptos::logging::error!("purge_system: {e}");
    }
    state.purging_system_id.set(-1);
}

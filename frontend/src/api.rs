//! GraphQL client and typed query/mutation helpers.
//!
//! Queries POST to the same-origin `/graphql` endpoint served by the Axum
//! backend. Data-loading helpers take `AppState` and publish what they fetch
//! into its source signals; deriving the filtered and paginated views from
//! those is the job of the memos in `state.rs`.

use gloo_net::http::Request;
use leptos::prelude::*;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::model::{Game, Info, NotificationKind, Rom, Setting, System};
use crate::notify::push_notification;
use crate::state::AppState;

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
            }
            Err(e) => report_error(state, "Loading systems", &e),
        },
        Err(e) => report_error(state, "Loading systems", &e),
    }
    state.loading_systems.set(false);
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
            }
            Err(e) => report_error(state, "Loading games", &e),
        },
        Err(e) => report_error(state, "Loading games", &e),
    }
    state.loading_games.set(false);
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
            }
            Err(e) => report_error(state, "Loading ROMs", &e),
        },
        Err(e) => report_error(state, "Loading ROMs", &e),
    }
    state.loading_roms.set(false);
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

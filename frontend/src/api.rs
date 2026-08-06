//! GraphQL client and typed query/mutation helpers.
//!
//! Queries POST to the same-origin `/graphql` endpoint served by the Axum
//! backend. Data-loading helpers take `AppState` and publish what they fetch
//! into its source signals; deriving the filtered and paginated views from
//! those is the job of the memos in `state.rs`.

use std::ops::ControlFlow;

use gloo_net::http::Request;
use leptos::prelude::*;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::model::{Game, Info, NotificationKind, Rom, Setting, Sizes, System};
use crate::notify::{Notifier, push_notification};
use crate::state::AppState;

/// The shape every GraphQL response arrives in.
#[derive(Deserialize)]
struct GraphQlResponse<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Vec<GraphQlError>,
}

#[derive(Deserialize)]
struct GraphQlError {
    message: String,
}

/// POST a GraphQL document and deserialize its `data` into `T`.
///
/// `T` is the caller's own struct rather than a `serde_json::Value`, so the
/// body is parsed exactly once. Going through `Value` meant allocating the
/// whole response as a tree and then walking it again to build the real types,
/// which for a large system is several megabytes of pointless work.
async fn graphql<T: DeserializeOwned>(query: &str, variables: Value) -> Result<T, String> {
    let body = json!({ "query": query, "variables": variables });
    let response = Request::post("/graphql")
        .json(&body)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let response: GraphQlResponse<T> = response.json().await.map_err(|e| e.to_string())?;
    // Surface the first message rather than the raw errors array, which is what
    // ends up in front of the user.
    if let Some(error) = response.errors.into_iter().next() {
        return Err(error.message);
    }
    response
        .data
        .ok_or_else(|| "the response contained no data".to_string())
}

/// Log a failed request and tell the user about it.
///
/// Without this the UI silently renders empty tables when the backend is
/// unreachable or a query fails.
pub fn report_error(notifier: Notifier, action: &str, error: &str) {
    leptos::logging::error!("{action}: {error}");
    push_notification(
        notifier,
        format!("{action} failed: {error}"),
        NotificationKind::Error,
    );
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
    graphql(query, Value::Null).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SystemSettingsData {
    system_settings: Vec<Setting>,
}

#[derive(Deserialize)]
struct SettingsData {
    settings: Vec<Setting>,
}

pub async fn get_raw_settings(system_id: Option<i64>) -> Result<Vec<Setting>, String> {
    match system_id {
        Some(id) => {
            let query = format!("{{ systemSettings(systemId: {}) {{ key value }} }}", id);
            let data: SystemSettingsData = graphql(&query, Value::Null).await?;
            Ok(data.system_settings)
        }
        None => {
            let query = "{ settings { key value } }";
            let data: SettingsData = graphql(query, Value::Null).await?;
            Ok(data.settings)
        }
    }
}

#[derive(Deserialize)]
struct SystemsData {
    systems: Vec<System>,
}

/// Fetch every system.
pub async fn fetch_systems(notifier: Notifier) -> Vec<System> {
    let query = r#"{
        systems { id name description completion merging arcade }
    }"#;
    match graphql::<SystemsData>(query, Value::Null).await {
        Ok(data) => data.systems,
        Err(e) => {
            report_error(notifier, "Loading systems", &e);
            Vec::new()
        }
    }
}

#[derive(Deserialize)]
struct GamesData {
    games: Vec<Game>,
}

/// How many games to ask for at a time.
///
/// Big enough that a console system arrives in one request, small enough that
/// an arcade set starts appearing almost immediately. The per request overhead
/// is about two milliseconds, so the extra round trips cost nothing next to
/// the wait they remove.
const GAMES_CHUNK: i64 = 10_000;

async fn fetch_games_chunk(
    notifier: Notifier,
    system_id: i64,
    offset: i64,
    limit: i64,
) -> Vec<Game> {
    let query = format!(
        "{{ games(systemId: {system_id}, offset: {offset}, limit: {limit}) \
         {{ id name description completion sorting }} }}"
    );
    match graphql::<GamesData>(&query, Value::Null).await {
        Ok(data) => {
            let mut games = data.games;
            for game in &mut games {
                game.name_lower = game.name.to_lowercase();
            }
            games
        }
        Err(e) => {
            report_error(notifier, "Loading games", &e);
            Vec::new()
        }
    }
}

/// Fetch a system's games a chunk at a time, handing each one to `receive` as
/// it lands so the list can be browsed while the rest is still coming.
///
/// `receive` returns [`ControlFlow::Break`] to abandon the rest, which is how
/// the caller drops a stream whose system is no longer selected.
pub async fn stream_games(
    notifier: Notifier,
    system_id: i64,
    mut receive: impl FnMut(Vec<Game>) -> ControlFlow<()>,
) {
    if system_id < 0 {
        return;
    }
    let mut offset = 0;
    loop {
        let chunk = fetch_games_chunk(notifier, system_id, offset, GAMES_CHUNK).await;
        let received = chunk.len() as i64;
        if receive(chunk).is_break() {
            return;
        }
        // A short chunk means we reached the end (or the request failed, which
        // has already been reported).
        if received < GAMES_CHUNK {
            return;
        }
        offset += received;
    }
}

#[derive(Deserialize)]
struct RomsData {
    roms: Vec<Rom>,
}

/// Fetch the roms of a game, or nothing when no game or system is selected.
pub async fn fetch_roms(notifier: Notifier, game_id: i64, system_id: i64) -> Vec<Rom> {
    if game_id < 0 || system_id < 0 {
        return Vec::new();
    }
    let query = format!(
        "{{ roms(gameId: {}) {{ id name size romfile {{ id path size }} ignored(systemId: {}) }} }}",
        game_id, system_id
    );
    match graphql::<RomsData>(&query, Value::Null).await {
        Ok(data) => data.roms,
        Err(e) => {
            report_error(notifier, "Loading ROMs", &e);
            Vec::new()
        }
    }
}

/// Fetch the aggregate sizes of a system.
pub async fn fetch_sizes(notifier: Notifier, system_id: i64) -> Sizes {
    if system_id < 0 {
        return Sizes::default();
    }
    let query = format!(
        "{{ totalOriginalSize(systemId: {id}) oneRegionOriginalSize(systemId: {id}) totalActualSize(systemId: {id}) oneRegionActualSize(systemId: {id}) }}",
        id = system_id
    );
    match graphql::<Sizes>(&query, Value::Null).await {
        Ok(sizes) => sizes,
        Err(e) => {
            report_error(notifier, "Loading statistics", &e);
            Sizes::default()
        }
    }
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
    .map(|_: serde::de::IgnoredAny| ())
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
    .map(|_: serde::de::IgnoredAny| ())
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
    .map(|_: serde::de::IgnoredAny| ())
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
    .map(|_: serde::de::IgnoredAny| ())
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
    .map(|_: serde::de::IgnoredAny| ())
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
    .map(|_: serde::de::IgnoredAny| ())
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
    .map(|_: serde::de::IgnoredAny| ())
}

pub async fn purge_system(state: AppState, system_id: i64) {
    state.purging_system_id.set(system_id);
    let mutation = r#"mutation PurgeSystem($systemId: Int!) {
        purgeSystem(systemId: $systemId)
    }"#;
    if let Err(e) =
        graphql::<serde::de::IgnoredAny>(mutation, json!({ "systemId": system_id })).await
    {
        report_error(state.notifier, "Purging the system", &e);
    }
    state.purging_system_id.set(-1);
}

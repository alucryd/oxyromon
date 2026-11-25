//! Data types mirroring the GraphQL API responses (see `src/query.rs` on the
//! backend). Only the fields actually requested by the frontend are modeled.

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct System {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub completion: i64,
    pub merging: i64,
    pub arcade: bool,
    /// Games in the system that are complete (not trashed).
    pub games_complete: i64,
    /// Games in the system that are not trashed.
    pub games_total: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Hash)]
pub struct Game {
    pub id: i64,
    pub name: String,
    /// Null when the backend would only be repeating `name`.
    pub description: Option<String>,
    pub completion: i64,
    pub sorting: i64,
    /// Lowercased `name`, filled in once when the games are fetched. Not part
    /// of the API response; it exists so the name filter does not have to
    /// reallocate a lowercase copy of every game name on every keystroke.
    #[serde(skip)]
    pub name_lower: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Hash)]
pub struct Romfile {
    pub id: i64,
    pub path: String,
    pub size: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Hash)]
pub struct Rom {
    pub id: i64,
    pub name: String,
    pub size: i64,
    pub crc: Option<String>,
    pub md5: Option<String>,
    pub sha1: Option<String>,
    pub romfile: Option<Romfile>,
    /// Only the games list's own query fills this in; a fetch that leaves it
    /// out (the game info modal's) reads it as false.
    #[serde(default)]
    pub ignored: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Setting {
    pub key: String,
    pub value: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Dependency {
    pub name: String,
    pub version: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub version: String,
    pub dependencies: Vec<Dependency>,
    pub system_count: i64,
    pub game_count: i64,
    pub rom_count: i64,
}

/// The four aggregate sizes reported for the current selection. The store's
/// own figures are not part of it — they describe the store, not the
/// selection, and live in [`StoreInfo`] for the system's info modal.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Sizes {
    pub total_original_size: i64,
    pub one_region_original_size: i64,
    pub total_actual_size: i64,
    pub one_region_actual_size: i64,
}

/// One system, with the figures its info modal shows and no other list
/// needs: the DAT it was imported from, and the four sizes. Mirrors the
/// response shape of `fetch_system_details`: the system's own facts are
/// nested under `system`, the four sizes are root siblings, and the store
/// answers under the server's own key, `systemStore`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SystemDetails {
    pub system: SystemFacts,
    pub total_original_size: i64,
    pub one_region_original_size: i64,
    pub total_actual_size: i64,
    pub one_region_actual_size: i64,
    #[serde(rename = "systemStore")]
    pub store: Option<StoreInfo>,
}

/// The system part of the info modal's payload: what the row menu's query
/// selects under `system`, and nothing the lists already carry. `name` and
/// `version` are the DAT it was imported from — the system row keeps both;
/// the GraphQL `header` field is the header rule, which most DATs have none of.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SystemFacts {
    pub name: String,
    pub version: String,
    pub games_complete: i64,
    pub games_total: i64,
    pub completion: i64,
}

/// The ORE store beneath a system: its path and weight, what it saves and
/// costs as a whole, this system's part of both, and every system that
/// shares it.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StoreInfo {
    pub path: String,
    pub size: i64,
    pub dedup_saved: i64,
    pub parity_cost: i64,
    pub system_dedup_saved: i64,
    pub system_parity_cost: i64,
    pub systems: Vec<String>,
}

/// What one game weighs: what its DAT declares, what it weighs on disk,
/// and — its files being OREs — what it owns, shares, and would weigh
/// detached.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GameSizes {
    pub original_size: i64,
    pub actual_size: i64,
    pub ore_own: Option<i64>,
    pub ore_share: Option<i64>,
    pub ore_standalone: Option<i64>,
}

/// Notification shown in the bell dropdown / toast.
#[derive(Clone, Debug, PartialEq)]
pub struct Notification {
    pub id: u64,
    pub message: String,
    pub kind: NotificationKind,
    /// When it arrived, ISO 8601. Stored as an instant rather than a formatted
    /// string so the bell can show it as "3 minutes ago" and keep that current.
    pub time: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationKind {
    Info,
    Success,
    Warning,
    Error,
}

impl NotificationKind {
    /// The Web Awesome colour variant that carries this meaning.
    pub fn variant(self) -> &'static str {
        match self {
            NotificationKind::Info => "brand",
            NotificationKind::Success => "success",
            NotificationKind::Warning => "warning",
            NotificationKind::Error => "danger",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            NotificationKind::Info => "circle-info",
            NotificationKind::Success => "circle-check",
            NotificationKind::Warning => "triangle-exclamation",
            NotificationKind::Error => "circle-exclamation",
        }
    }
}

//! Data types mirroring the GraphQL API responses (see `src/query.rs` on the
//! backend). Only the fields actually requested by the frontend are modeled.

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct System {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub completion: i64,
    pub merging: i64,
    pub arcade: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Game {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub completion: i64,
    pub sorting: i64,
    /// Lowercased `name`, filled in once when the games are fetched. Not part
    /// of the API response; it exists so the name filter does not have to
    /// reallocate a lowercase copy of every game name on every keystroke.
    #[serde(skip)]
    pub name_lower: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Romfile {
    pub id: i64,
    pub path: String,
    pub size: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Rom {
    pub id: i64,
    pub name: String,
    pub size: i64,
    pub romfile: Option<Romfile>,
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

/// The four aggregate sizes reported for a system.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sizes {
    pub total_original: i64,
    pub one_region_original: i64,
    pub total_actual: i64,
    pub one_region_actual: i64,
}

/// Notification shown in the bell dropdown / toast.
#[derive(Clone, Debug, PartialEq)]
pub struct Notification {
    pub id: u64,
    pub message: String,
    pub kind: NotificationKind,
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
    pub fn dot_class(self) -> &'static str {
        match self {
            NotificationKind::Info => "bg-sky-400",
            NotificationKind::Success => "bg-emerald-400",
            NotificationKind::Warning => "bg-amber-400",
            NotificationKind::Error => "bg-rose-400",
        }
    }
}

// Parameter structs for the MCP tools and prompts. The `#[tool]` and
// `#[prompt]` macros derive each tool's JSON Schema from the field names,
// types, and doc comments here — the doc comments are what the model reads.

use rmcp::schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SystemIdParams {
    /// The system id, as returned by list_systems.
    pub system_id: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct GameIdParams {
    /// The game id, as returned by list_games or search_games.
    pub game_id: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ListGamesParams {
    /// The system id, as returned by list_systems.
    pub system_id: i64,
    /// Skip this many games (ordered by name). Defaults to 0.
    #[serde(default)]
    pub offset: Option<i64>,
    /// Return at most this many games. Defaults to 100, capped at 1000.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Optional case-insensitive substring filter on the game name.
    #[serde(default)]
    pub search: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SearchGamesParams {
    /// Case-insensitive substring to look for in game names, across all systems.
    pub query: String,
    /// Return at most this many games. Defaults to 50, capped at 500.
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct GetSettingsParams {
    /// Read the settings as they apply to this system (its overrides merged
    /// over the globals). Omit for the global settings.
    #[serde(default)]
    pub system_id: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DownloadableSystemsParams {
    /// Also list already-imported systems whose DAT files can be refreshed.
    #[serde(default)]
    pub update: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ParseGameNameParams {
    /// A No-Intro style game file or ROM name to parse, e.g.
    /// "Super Mario World (USA) (Rev 1)".
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ImportDatsParams {
    /// Paths of the DAT files (or ZIP archives of DATs) to import. The paths
    /// are resolved on the machine oxyromon runs on.
    pub paths: Vec<String>,
    /// Update existing systems from the DATs instead of skipping them.
    #[serde(default)]
    pub update: Option<bool>,
    /// Import the systems as arcade, for DATs that list games instead of
    /// machines (FinalBurn Neo, for example). DATs with machines are always
    /// arcade. An update keeps a system arcade once it is.
    #[serde(default)]
    pub arcade: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DownloadDatsParams {
    /// Exact Redump system names, as returned by downloadable_systems.
    pub systems: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ImportRomsParams {
    /// Paths of the ROM files or directories to import, on the machine
    /// oxyromon runs on.
    pub paths: Vec<String>,
    /// Restrict matching to this system, as returned by list_systems.
    #[serde(default)]
    pub system_id: Option<i64>,
    /// How to handle ROMs that match several games: pick the "first" match or
    /// "skip" them. Defaults to "first".
    #[serde(default)]
    pub unattended: Option<Unattended>,
    /// Import files as-is without converting them to the preferred format.
    #[serde(default)]
    pub as_is: Option<bool>,
    /// Extract top-level archives before importing their contents.
    #[serde(default)]
    pub extract: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Unattended {
    /// Skip files that need a human to choose.
    Skip,
    /// Auto-select the first match.
    First,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SortRomsParams {
    /// Sort only this system. Mutually exclusive with "all".
    #[serde(default)]
    pub system_id: Option<i64>,
    /// Sort every system.
    #[serde(default)]
    pub all: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CheckRomsParams {
    /// Check only this system. Mutually exclusive with "all".
    #[serde(default)]
    pub system_id: Option<i64>,
    /// Check every system.
    #[serde(default)]
    pub all: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ConvertRomsParams {
    /// Convert this system's ROM files.
    pub system_id: i64,
    /// Target format: "ORIGINAL", "7Z", "CHD", "CSO", "NSZ", "RVZ", "ZIP" or
    /// "ZSO" — the same values as PREFER_FORMAT.
    pub format: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SetSettingParams {
    /// The setting key, e.g. "PREFER_FORMAT", "PREFER_PARENTS",
    /// "REGIONS_ONE_SUBFOLDERS". get_settings lists the keys in use.
    pub key: String,
    /// The new value as a string ("true"/"false" for booleans, a number for
    /// integers, a single choice for enums). For list settings, a
    /// comma-separated value replaces the whole list. Omit to unset a nullable
    /// setting.
    #[serde(default)]
    pub value: Option<String>,
    /// Apply the override to this system only; omit for the global setting.
    #[serde(default)]
    pub system_id: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PurgeRomsParams {
    /// Forget ROM file records whose file is missing on disk.
    #[serde(default)]
    pub missing: bool,
    /// Forget ROM file records that no imported ROM references.
    #[serde(default)]
    pub orphan: bool,
    /// Delete the files currently in the Trash directories.
    #[serde(default)]
    pub trash: bool,
    /// Delete files in the ROM directories that oxyromon does not know.
    #[serde(default)]
    pub foreign: bool,
    /// Must be true when trash or foreign is set: the file deletions cannot
    /// be undone.
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PurgeSystemParams {
    /// Purge this system: its files move to its Trash directory.
    pub system_id: i64,
    /// Must be true to proceed.
    #[serde(default)]
    pub confirm: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PurgeIrdsParams {
    /// Purge every IRD (JB folder) game of this system.
    pub system_id: i64,
    /// Must be true to proceed.
    #[serde(default)]
    pub confirm: bool,
}

// ── prompts ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct OnboardSystemArgs {
    /// The system to onboard, e.g. "Sony - PlayStation".
    pub system: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct AuditSystemArgs {
    /// The system id to audit, as returned by list_systems.
    pub system_id: i64,
}

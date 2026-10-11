// The JSON shapes the tools and resources return. They mirror the model row
// types but stay compact (snake_case, enum names instead of integers, no
// bookkeeping ids a caller can't use), because every field lands in a model's
// context window.

use serde::Serialize;
use sqlx::FromRow;

pub(crate) fn merging_name(merging: i64) -> &'static str {
    match merging {
        0 => "split",
        1 => "non-merged",
        2 => "full-non-merged",
        3 => "merged",
        4 => "full-merged",
        _ => "unknown",
    }
}

pub(crate) fn sorting_name(sorting: i64) -> &'static str {
    match sorting {
        0 => "all-regions",
        1 => "one-region",
        2 => "ignored",
        _ => "unknown",
    }
}

pub(crate) fn completion_name(completion: i64) -> &'static str {
    match completion {
        0 => "missing",
        1 => "partial",
        2 => "complete",
        _ => "unknown",
    }
}

pub(crate) fn romfile_type_name(romfile_type: i64) -> &'static str {
    match romfile_type {
        0 => "romfile",
        1 => "playlist",
        2 => "patch",
        _ => "unknown",
    }
}

#[derive(Serialize)]
pub(crate) struct SystemOut {
    pub id: i64,
    pub name: String,
    pub version: String,
    pub description: String,
    /// Catalog completion of the imported set, 0–100.
    pub completion: i64,
    pub arcade: bool,
    pub merging: &'static str,
    pub games_complete: i64,
    pub games_total: i64,
}

impl SystemOut {
    pub(crate) fn from_system(system: &super::super::model::System) -> Self {
        SystemOut {
            id: system.id,
            name: system.name.clone(),
            version: system.version.clone(),
            description: system.description.clone(),
            completion: system.completion,
            arcade: system.arcade,
            merging: merging_name(system.merging),
            games_complete: system.games_complete,
            games_total: system.games_total,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct GameOut {
    pub id: i64,
    pub name: String,
    pub regions: String,
    /// How the game is filed by sort-roms.
    pub sorting: &'static str,
    /// Whether the game's expected ROM files are all imported.
    pub completion: &'static str,
    pub bios: bool,
    pub device: bool,
    pub system_id: i64,
}

impl GameOut {
    pub(crate) fn from_game(game: &super::super::model::Game) -> Self {
        GameOut {
            id: game.id,
            name: game.name.clone(),
            regions: game.regions.clone(),
            sorting: sorting_name(game.sorting),
            completion: completion_name(game.completion),
            bios: game.bios,
            device: game.device,
            system_id: game.system_id,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct RomfileOut {
    pub id: i64,
    /// Path relative to the ROM directory.
    pub path: String,
    pub size: i64,
    pub romfile_type: &'static str,
}

#[derive(Serialize)]
pub(crate) struct RomOut {
    pub id: i64,
    pub name: String,
    pub size: i64,
    pub crc: Option<String>,
    pub md5: Option<String>,
    pub sha1: Option<String>,
    /// Expected status from the catalog: "ok", "baddump", "nodump", …
    pub status: Option<String>,
    /// The file on disk matching this expected ROM, if imported.
    pub romfile: Option<RomfileOut>,
}

#[derive(Serialize)]
pub(crate) struct GameDetailOut {
    #[serde(flatten)]
    pub game: GameOut,
    pub system_name: String,
    pub roms: Vec<RomOut>,
}

#[derive(Serialize, FromRow)]
pub(crate) struct SearchHitOut {
    pub game_id: i64,
    pub name: String,
    pub system_id: i64,
    pub system_name: String,
}

#[derive(Serialize)]
pub(crate) struct SettingsOut {
    pub settings: Vec<SettingOut>,
}

#[derive(Serialize)]
pub(crate) struct SettingOut {
    pub key: String,
    pub value: Option<String>,
    /// None for the global value, Some for a per-system override.
    pub system_id: Option<i64>,
}

#[derive(Serialize)]
pub(crate) struct DependencyOut {
    pub name: String,
    /// None when the dependency is not available.
    pub version: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct StatusOut {
    pub version: String,
    pub read_only: bool,
    /// The directory ROM files live under (paths in results are relative to it).
    pub rom_directory: String,
    pub tmp_directory: String,
    pub systems: i64,
    pub games: i64,
    pub roms: i64,
    pub dependencies: Vec<DependencyOut>,
}

#[derive(Serialize)]
pub(crate) struct SystemDetailOut {
    #[serde(flatten)]
    pub system: SystemOut,
    /// Sum of the expected sizes of imported ROMs.
    pub total_original_size: i64,
    /// Same, counting only the 1G1R selection.
    pub one_region_original_size: i64,
    /// Sum of the actual file sizes on disk.
    pub total_actual_size: i64,
    /// Same, counting only the 1G1R selection.
    pub one_region_actual_size: i64,
}

#[derive(Serialize)]
pub(crate) struct PagedGamesOut {
    pub system_id: i64,
    pub offset: i64,
    pub returned: usize,
    /// Total games of the system (unfiltered).
    pub total: i64,
    pub games: Vec<GameOut>,
}

#[derive(Serialize)]
pub(crate) struct ActionOut {
    /// What ran, for echoing back into the conversation.
    pub action: String,
    /// Tool-level detail, e.g. counts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

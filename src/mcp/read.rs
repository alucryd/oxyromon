// The read surface: catalog queries an assistant uses to understand the
// library before touching it. Everything here is `readOnlyHint` and needs no
// write lock.

use super::actions::{ok_json, tool_error};
use super::output::{
    DependencyOut, GameDetailOut, GameOut, PagedGamesOut, RomOut, RomfileOut, SearchHitOut,
    SettingOut, SettingsOut, StatusOut, SystemDetailOut, SystemOut, romfile_type_name,
};
use super::params::{
    DownloadableSystemsParams, GameIdParams, GetSettingsParams, ListGamesParams,
    ParseGameNameParams, SearchGamesParams, SystemIdParams,
};
use super::{OxyromonMcp, internal};
use crate::config::{get_rom_directory, get_tmp_directory};
use crate::database::*;
use crate::download_dats::find_redump_systems;
use crate::model::{Game, Rom, Romfile};
use itertools::Itertools;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use serde_json::json;
use shiratsu_naming::naming::TokenizedName;
use shiratsu_naming::naming::nointro::{NoIntroName, NoIntroToken};
use shiratsu_naming::region::Region;
use sqlx::{AssertSqlSafe, SqliteConnection};

const DEFAULT_GAMES_LIMIT: i64 = 100;
const MAX_GAMES_LIMIT: i64 = 1000;
const DEFAULT_SEARCH_LIMIT: i64 = 50;
const MAX_SEARCH_LIMIT: i64 = 500;

/// Mirrors the GraphQL `system_size` resolver: expected vs actual bytes, with
/// and without the 1G1R selection. The id is a number interpolated after the
/// existence check, so the SQL stays safe; sizes are cheap SUMs.
async fn system_size(
    connection: &mut SqliteConnection,
    system_id: i64,
    actual: bool,
    one_region: bool,
) -> Result<i64, String> {
    let sorting = if one_region { "AND g.sorting = 1" } else { "" };
    let sql = if actual {
        format!(
            "SELECT COALESCE(SUM(rf.size), 0) FROM romfiles AS rf WHERE rf.id IN (
                 SELECT DISTINCT(r.romfile_id) FROM roms AS r
                 JOIN games AS g ON r.game_id = g.id
                 WHERE r.romfile_id IS NOT NULL {sorting} AND g.system_id = {system_id})"
        )
    } else {
        format!(
            "SELECT COALESCE(SUM(r.size), 0) FROM roms AS r
             JOIN games AS g ON r.game_id = g.id
             WHERE r.romfile_id IS NOT NULL {sorting} AND g.system_id = {system_id}"
        )
    };
    let row: (i64,) = sqlx::query_as(AssertSqlSafe(sql.as_str()))
        .fetch_one(&mut *connection)
        .await
        .map_err(|e| format!("failed to compute sizes: {e}"))?;
    Ok(row.0)
}

/// Build the status payload; shared by the `status` tool and the
/// `oxyromon://status` resource.
pub(crate) async fn build_status(connection: &mut SqliteConnection, read_only: bool) -> StatusOut {
    let mut dependencies = vec![
        ("sevenz-rust2", crate::archive::get_version().await),
        ("chd-rs", crate::chd::get_version().await),
        ("cia-rs", crate::cia::get_version().await),
        ("xps-rs", crate::xps::get_version().await),
        ("xso-rs", crate::xso::get_version().await),
        ("nsz-rs", crate::nsz::get_version().await),
        ("xdelta-rs", crate::xdelta::get_version().await),
        ("wux-rs", crate::wux::get_version().await),
        // RVZ and WBFS both come from nod
        ("nod", crate::rvz::get_version().await),
    ];
    dependencies.sort_by_key(|(name, _)| *name);
    StatusOut {
        version: env!("CARGO_PKG_VERSION").to_string(),
        read_only,
        rom_directory: get_rom_directory(connection)
            .await
            .to_string_lossy()
            .to_string(),
        tmp_directory: get_tmp_directory(connection)
            .await
            .to_string_lossy()
            .to_string(),
        systems: count_systems(connection).await,
        games: count_games(connection).await,
        roms: count_roms(connection).await,
        dependencies: dependencies
            .into_iter()
            .map(|(name, version)| DependencyOut {
                name: name.to_string(),
                version: version.ok(),
            })
            .collect(),
    }
}

#[tool_router(router = read_router, vis = "pub(crate)")]
impl OxyromonMcp {
    #[tool(
        description = "oxyROMon status: version, ROM and temp directories, library counts, and which codecs and format backends are available.",
        annotations(read_only_hint = true)
    )]
    async fn status(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        let mut conn = self.pool.acquire().await.map_err(internal)?;
        let connection = &mut *conn;
        let status = build_status(connection, self.read_only).await;
        Ok(ok_json(&status))
    }

    #[tool(
        description = "List every system in the library with its DAT version, catalog completion, and complete/total game counts.",
        annotations(read_only_hint = true)
    )]
    async fn list_systems(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        let mut conn = self.pool.acquire().await.map_err(internal)?;
        let connection = &mut *conn;
        let systems: Vec<SystemOut> = find_systems(connection)
            .await
            .iter()
            .map(SystemOut::from_system)
            .collect();
        let count = systems.len();
        Ok(ok_json(&json!({ "count": count, "systems": systems })))
    }

    #[tool(
        description = "One system with its stats: game counts, completion, and the expected vs actual size of its imported ROMs, all and 1G1R only.",
        annotations(read_only_hint = true)
    )]
    async fn get_system(
        &self,
        Parameters(params): Parameters<SystemIdParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let mut conn = self.pool.acquire().await.map_err(internal)?;
        let connection = &mut *conn;
        let Some(system) = find_system_by_id_opt(connection, params.system_id).await else {
            return Ok(tool_error(format!(
                "system {} not found; list_systems has the ids",
                params.system_id
            )));
        };
        let detail = SystemDetailOut {
            system: SystemOut::from_system(&system),
            total_original_size: system_size(connection, system.id, false, false)
                .await
                .map_err(internal)?,
            one_region_original_size: system_size(connection, system.id, false, true)
                .await
                .map_err(internal)?,
            total_actual_size: system_size(connection, system.id, true, false)
                .await
                .map_err(internal)?,
            one_region_actual_size: system_size(connection, system.id, true, true)
                .await
                .map_err(internal)?,
        };
        Ok(ok_json(&detail))
    }

    #[tool(
        description = "Games of a system, ordered by name, paged (offset/limit; defaults: limit 100). An optional `search` filters on name substring.",
        annotations(read_only_hint = true)
    )]
    async fn list_games(
        &self,
        Parameters(params): Parameters<ListGamesParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let mut conn = self.pool.acquire().await.map_err(internal)?;
        let connection = &mut *conn;
        if find_system_by_id_opt(connection, params.system_id)
            .await
            .is_none()
        {
            return Ok(tool_error(format!(
                "system {} not found; list_systems has the ids",
                params.system_id
            )));
        }
        let offset = params.offset.unwrap_or(0).max(0);
        let limit = params
            .limit
            .unwrap_or(DEFAULT_GAMES_LIMIT)
            .clamp(1, MAX_GAMES_LIMIT);
        let games: Vec<Game> = match &params.search {
            Some(search) => {
                sqlx::query_as::<_, Game>(
                    "SELECT * FROM games WHERE system_id = ? AND name LIKE ? ORDER BY name LIMIT ? OFFSET ?",
                )
                .bind(params.system_id)
                .bind(format!("%{search}%"))
                .bind(limit)
                .bind(offset)
                .fetch_all(&mut *connection)
                .await
                .map_err(internal)?
            }
            None => {
                sqlx::query_as::<_, Game>(
                    "SELECT * FROM games WHERE system_id = ? ORDER BY name LIMIT ? OFFSET ?",
                )
                .bind(params.system_id)
                .bind(limit)
                .bind(offset)
                .fetch_all(&mut *connection)
                .await
                .map_err(internal)?
            }
        };
        let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM games WHERE system_id = ?")
            .bind(params.system_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(internal)?;
        let returned = games.len();
        let out = PagedGamesOut {
            system_id: params.system_id,
            offset,
            returned,
            total: total.0,
            games: games.iter().map(GameOut::from_game).collect(),
        };
        Ok(ok_json(&out))
    }

    #[tool(
        description = "One game with its system name and every expected ROM file: hashes, status, and the file on disk matching it (path relative to the ROM directory) when imported.",
        annotations(read_only_hint = true)
    )]
    async fn get_game(
        &self,
        Parameters(params): Parameters<GameIdParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let mut conn = self.pool.acquire().await.map_err(internal)?;
        let connection = &mut *conn;
        let games: Vec<Game> = sqlx::query_as::<_, Game>("SELECT * FROM games WHERE id = ?")
            .bind(params.game_id)
            .fetch_all(&mut *connection)
            .await
            .map_err(internal)?;
        let Some(game) = games.first() else {
            return Ok(tool_error(format!(
                "game {} not found; list_games has the ids",
                params.game_id
            )));
        };
        let system = find_system_by_id_opt(connection, game.system_id)
            .await
            .ok_or_else(|| internal("game points at a missing system"))?;
        let roms: Vec<Rom> = find_roms_by_game_id_parents(connection, game.id).await;
        let mut rom_outs = Vec::with_capacity(roms.len());
        for rom in &roms {
            let romfile: Option<Romfile> = match rom.romfile_id {
                Some(romfile_id) => Some(find_romfile_by_id(connection, romfile_id).await),
                None => None,
            };
            rom_outs.push(RomOut {
                id: rom.id,
                name: rom.name.clone(),
                size: rom.size,
                crc: rom.crc.clone(),
                md5: rom.md5.clone(),
                sha1: rom.sha1.clone(),
                status: rom.rom_status.clone(),
                romfile: romfile.map(|romfile| RomfileOut {
                    id: romfile.id,
                    path: romfile.path,
                    size: romfile.size,
                    romfile_type: romfile_type_name(romfile.romfile_type),
                }),
            });
        }
        let detail = GameDetailOut {
            game: GameOut::from_game(game),
            system_name: system.name,
            roms: rom_outs,
        };
        Ok(ok_json(&detail))
    }

    #[tool(
        description = "Search game names across every system (case-insensitive substring). Returns game and system ids.",
        annotations(read_only_hint = true)
    )]
    async fn search_games(
        &self,
        Parameters(params): Parameters<SearchGamesParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let mut conn = self.pool.acquire().await.map_err(internal)?;
        let connection = &mut *conn;
        let limit = params
            .limit
            .unwrap_or(DEFAULT_SEARCH_LIMIT)
            .clamp(1, MAX_SEARCH_LIMIT);
        let hits: Vec<SearchHitOut> = sqlx::query_as::<_, SearchHitOut>(
            "SELECT g.id AS game_id, g.name, g.system_id, s.name AS system_name
             FROM games AS g JOIN systems AS s ON s.id = g.system_id
             WHERE g.name LIKE ? ORDER BY s.name, g.name LIMIT ?",
        )
        .bind(format!("%{}%", params.query))
        .bind(limit)
        .fetch_all(&mut *connection)
        .await
        .map_err(internal)?;
        let count = hits.len();
        Ok(ok_json(&json!({ "count": count, "games": hits })))
    }

    #[tool(
        description = "Settings with their current values. With system_id, the values as that system sees them (per-system overrides merged over the globals).",
        annotations(read_only_hint = true)
    )]
    async fn get_settings(
        &self,
        Parameters(params): Parameters<GetSettingsParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let mut conn = self.pool.acquire().await.map_err(internal)?;
        let connection = &mut *conn;
        let mut merged: std::collections::HashMap<String, SettingOut> =
            find_settings(connection, None)
                .await
                .into_iter()
                .map(|setting| {
                    (
                        setting.key.clone(),
                        SettingOut {
                            key: setting.key,
                            value: setting.value,
                            system_id: None,
                        },
                    )
                })
                .collect();
        if let Some(system_id) = params.system_id {
            for setting in find_settings(connection, Some(system_id)).await {
                merged.insert(
                    setting.key.clone(),
                    SettingOut {
                        key: setting.key,
                        value: setting.value,
                        system_id: Some(system_id),
                    },
                );
            }
        }
        let mut settings: Vec<SettingOut> = merged.into_values().collect();
        settings.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(ok_json(&SettingsOut { settings }))
    }

    #[tool(
        description = "Redump systems the library can fetch with download_dats: not yet imported, or — with update: true — already imported and refreshable.",
        annotations(read_only_hint = true)
    )]
    async fn downloadable_systems(
        &self,
        Parameters(params): Parameters<DownloadableSystemsParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let mut conn = self.pool.acquire().await.map_err(internal)?;
        let connection = &mut *conn;
        let systems = find_redump_systems(connection, params.update.unwrap_or(false)).await;
        let count = systems.len();
        Ok(ok_json(&json!({ "count": count, "systems": systems })))
    }

    #[tool(
        description = "Parse a No-Intro style ROM name into title, regions, languages, release, and flags — useful to reason about disc/region variants.",
        annotations(read_only_hint = true)
    )]
    async fn parse_game_name(
        &self,
        Parameters(params): Parameters<ParseGameNameParams>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let mut title = String::new();
        let mut regions: Vec<String> = vec![];
        let mut languages: Vec<String> = vec![];
        let mut release: Option<String> = None;
        let mut flags: Vec<String> = vec![];

        if let Ok(name) = NoIntroName::try_parse(&params.name) {
            for token in name.iter() {
                match token {
                    NoIntroToken::Title(parsed_title) => title = parsed_title.to_string(),
                    NoIntroToken::Region {
                        region_strings: _,
                        regions: parsed_regions,
                    } => {
                        regions.append(
                            &mut Region::to_normalized_region_string(parsed_regions.as_slice())
                                .split('-')
                                .map(|region| region.to_string())
                                .collect_vec(),
                        );
                    }
                    NoIntroToken::Languages(parsed_languages) => {
                        languages.append(
                            &mut parsed_languages
                                .iter()
                                .map(|language| language.code.to_string())
                                .collect_vec(),
                        );
                    }
                    NoIntroToken::Release { status, number: _ } => {
                        release = Some(status.to_string());
                    }
                    NoIntroToken::Flag { flag_type: _, flag } => {
                        flags.append(
                            &mut flag.split(", ").map(|flag| flag.to_string()).collect_vec(),
                        );
                    }
                    _ => (),
                }
            }
        }
        Ok(ok_json(&json!({
            "name": params.name,
            "parsed": title,
            "regions": regions,
            "languages": languages,
            "release": release,
            "flags": flags,
        })))
    }
}

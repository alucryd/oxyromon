// The mutating surface. Every tool here runs through `run_action`: write lock,
// hidden progress bar bridged to MCP progress, cancellation, and SSE fan-out.
//
// Actions are the CLI subcommands themselves, reached the way the GraphQL
// mutations reach them — a synthesized `ArgMatches` — so CLI, GraphQL, and MCP
// share one implementation. Destructive tools are annotated and gated behind
// an explicit `confirm: true`.

use super::actions::{need_confirm, ok_json, ok_text, read_only_denial, tool_error};
use super::output::{ActionOut, DownloadDatsOut, FailedSystemOut};
use super::params::{
    CheckRomsParams, ConvertRomsParams, DownloadDatsParams, ImportDatsParams, ImportRomsParams,
    PurgeIrdsParams, PurgeRomsParams, PurgeSystemParams, SetSettingParams, SortRomsParams,
};
use super::resources::system_uri;
use super::{OxyromonMcp, internal};
use crate::check_roms;
use crate::config::{self, BOOLEANS, LISTS, NULLABLES, PATHS, choice_options, integer_range};
use crate::convert_roms;
use crate::database::*;
use crate::download_dats::download_redump_systems;
use crate::import_dats;
use crate::import_roms;
use crate::model::System;
use crate::progress::*;
use crate::purge_irds;
use crate::purge_roms;
use crate::purge_systems::purge_system;
use crate::sort_roms;
use anyhow::Result;
use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{tool, tool_router};

/// Resolve a tool's `system_id` to the system its CLI takes by name.
impl OxyromonMcp {
    /// The system, or a tool error naming the unknown id.
    async fn require_system(
        &self,
        system_id: i64,
    ) -> Result<Result<System, CallToolResult>, McpError> {
        let connection = &mut self.pool.acquire().await.map_err(internal)?;
        Ok(find_system_by_id_opt(connection, system_id)
            .await
            .ok_or_else(|| {
                tool_error(format!(
                    "system {system_id} not found; list_systems has the ids"
                ))
            }))
    }
}

#[tool_router(router = write_router, vis = "pub(crate)")]
impl OxyromonMcp {
    #[tool(
        description = "Import Logiqx DAT files (or ZIP archives of them), creating or refreshing systems and their game/ROM catalogs. Existing systems are skipped unless update: true. This defines what import_roms can match.",
        annotations(read_only_hint = false, idempotent_hint = true)
    )]
    async fn import_dats(
        &self,
        Parameters(params): Parameters<ImportDatsParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        if self.read_only {
            return Ok(read_only_denial("import_dats"));
        }
        if params.paths.is_empty() {
            return Ok(tool_error("no DAT paths given"));
        }
        let update = params.update.unwrap_or(false);
        let mut argv = vec!["import-dats".to_string()];
        if update {
            argv.push("--update".to_string());
        }
        argv.extend(params.paths);

        let outcome = self
            .run_action(
                "Importing DAT files".to_string(),
                Some("import_dats"),
                ctx.peer.clone(),
                ctx.meta.get_progress_token(),
                ctx.ct.clone(),
                move |connection, progress_bar| {
                    Box::pin(async move {
                        let matches = import_dats::subcommand().try_get_matches_from(argv)?;
                        import_dats::main(connection, &matches, progress_bar).await
                    })
                },
            )
            .await;
        if let Err(message) = outcome {
            return Ok(tool_error(message));
        }
        self.notify_resources_changed(&ctx.peer).await;
        Ok(ok_json(&ActionOut {
            action: "import_dats".to_string(),
            detail: None,
        }))
    }

    #[tool(
        description = "Download the DAT files of Redump systems from redump.org and import them — the systems listed by downloadable_systems. A DAT no newer than the imported one changes nothing; the result lists each system as imported, up_to_date or failed.",
        annotations(read_only_hint = false, idempotent_hint = true, open_world_hint = true)
    )]
    async fn download_dats(
        &self,
        Parameters(params): Parameters<DownloadDatsParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        if self.read_only {
            return Ok(read_only_denial("download_dats"));
        }
        if params.systems.is_empty() {
            return Ok(tool_error("no systems given"));
        }
        let count = params.systems.len();
        let outcome = self
            .run_action(
                format!("Downloading {count} system DAT files"),
                Some("download_dats"),
                ctx.peer.clone(),
                ctx.meta.get_progress_token(),
                ctx.ct.clone(),
                move |connection, progress_bar| {
                    Box::pin(async move {
                        start_action(progress_bar, Some("Downloading DAT files"));
                        // one failure does not cost the rest of the batch
                        let outcomes = download_redump_systems(
                            connection,
                            progress_bar,
                            &params.systems,
                            false,
                        )
                        .await;
                        Ok(DownloadDatsOut {
                            imported: outcomes.imported,
                            up_to_date: outcomes.up_to_date,
                            failed: outcomes
                                .failed
                                .into_iter()
                                .map(|(system, e)| FailedSystemOut {
                                    system,
                                    error: format!("{e:#}"),
                                })
                                .collect(),
                        })
                    })
                },
            )
            .await;
        let out = match outcome {
            Ok(out) => out,
            Err(message) => return Ok(tool_error(message)),
        };
        if !out.imported.is_empty() {
            self.notify_resources_changed(&ctx.peer).await;
        }
        if out.imported.is_empty() && out.up_to_date.is_empty() {
            let errors: Vec<String> = out
                .failed
                .iter()
                .map(|failed| format!("{}: {}", failed.system, failed.error))
                .collect();
            return Ok(tool_error(errors.join("; ")));
        }
        Ok(ok_json(&out))
    }

    #[tool(
        description = "Hash ROM files (or directories of them) and match them against the catalog, importing the matches. Unambiguous files import silently; `unattended` decides between skipping and auto-picking for ambiguous ones.",
        annotations(read_only_hint = false, idempotent_hint = true)
    )]
    async fn import_roms(
        &self,
        Parameters(params): Parameters<ImportRomsParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        if self.read_only {
            return Ok(read_only_denial("import_roms"));
        }
        if params.paths.is_empty() {
            return Ok(tool_error("no ROM paths given"));
        }
        let system = match params.system_id {
            Some(system_id) => match self.require_system(system_id).await? {
                Ok(system) => Some(system),
                Err(result) => return Ok(result),
            },
            None => None,
        };
        let unattended = match params.unattended {
            None | Some(super::params::Unattended::First) => "first",
            Some(super::params::Unattended::Skip) => "skip",
        };
        let mut argv = vec!["import-roms".to_string()];
        argv.extend(params.paths);
        if let Some(system) = &system {
            argv.extend(["--system".to_string(), system.name.clone()]);
        }
        argv.extend(["--unattended".to_string(), unattended.to_string()]);
        if params.as_is.unwrap_or(false) {
            argv.push("--as-is".to_string());
        }
        if params.extract.unwrap_or(false) {
            argv.push("--extract".to_string());
        }

        let system_id = system.as_ref().map(|system| system.id);
        let label = match &system {
            Some(system) => format!("Importing ROMs into \"{}\"", system.name),
            None => "Importing ROMs".to_string(),
        };
        let outcome = self
            .run_action(
                label,
                None,
                ctx.peer.clone(),
                ctx.meta.get_progress_token(),
                ctx.ct.clone(),
                move |connection, progress_bar| {
                    Box::pin(async move {
                        let matches = import_roms::subcommand().try_get_matches_from(argv)?;
                        import_roms::main(connection, &matches, progress_bar).await
                    })
                },
            )
            .await;
        if let Err(message) = outcome {
            return Ok(tool_error(message));
        }
        match system_id {
            Some(system_id) => {
                self.notify_resource_updated(&ctx.peer, &system_uri(system_id))
                    .await
            }
            None => self.notify_resources_changed(&ctx.peer).await,
        }
        Ok(ok_text(
            "import complete — use get_system or list_games for the new completion state",
        ))
    }

    #[tool(
        description = "File imported ROMs into their region folders, the 1G1R selection, and Trash, following the configured region/version/format preferences. Pass system_id for one system or all: true.",
        annotations(read_only_hint = false, idempotent_hint = true)
    )]
    async fn sort_roms(
        &self,
        Parameters(params): Parameters<SortRomsParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        if self.read_only {
            return Ok(read_only_denial("sort_roms"));
        }

        let (argv, label, system_id) = match (params.system_id, params.all.unwrap_or(false)) {
            (Some(system_id), false) => match self.require_system(system_id).await? {
                Ok(system) => (
                    vec![
                        "sort-roms".to_string(),
                        "--system".to_string(),
                        system.name.clone(),
                        "--yes".to_string(),
                    ],
                    format!("Sorting the ROMs of \"{}\"", system.name),
                    Some(system_id),
                ),
                Err(result) => return Ok(result),
            },
            (None, true) => (
                vec![
                    "sort-roms".to_string(),
                    "--all".to_string(),
                    "--yes".to_string(),
                ],
                "Sorting the ROMs of all systems".to_string(),
                None,
            ),
            _ => {
                return Ok(tool_error(
                    "pass system_id for one system, or all: true for every system",
                ));
            }
        };
        let outcome = self
            .run_action(
                label,
                Some("sort_roms"),
                ctx.peer.clone(),
                ctx.meta.get_progress_token(),
                ctx.ct.clone(),
                move |connection, progress_bar| {
                    Box::pin(async move {
                        let matches = sort_roms::subcommand().try_get_matches_from(argv)?;
                        sort_roms::main(connection, &matches, progress_bar).await
                    })
                },
            )
            .await;
        if let Err(message) = outcome {
            return Ok(tool_error(message));
        }
        match system_id {
            Some(system_id) => {
                self.notify_resource_updated(&ctx.peer, &system_uri(system_id))
                    .await
            }
            None => self.notify_resources_changed(&ctx.peer).await,
        }
        Ok(ok_text("sorting complete"))
    }

    #[tool(
        description = "Re-hash imported ROM files against the catalog, reporting and trashing mismatches. Pass system_id for one system or all: true.",
        annotations(read_only_hint = false, idempotent_hint = true)
    )]
    async fn check_roms(
        &self,
        Parameters(params): Parameters<CheckRomsParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        if self.read_only {
            return Ok(read_only_denial("check_roms"));
        }
        let (argv, label, system_id) = match (params.system_id, params.all.unwrap_or(false)) {
            (Some(system_id), false) => match self.require_system(system_id).await? {
                Ok(system) => (
                    vec![
                        "check-roms".to_string(),
                        "--system".to_string(),
                        system.name.clone(),
                    ],
                    format!("Checking the ROMs of \"{}\"", system.name),
                    Some(system_id),
                ),
                Err(result) => return Ok(result),
            },
            (None, true) => (
                vec!["check-roms".to_string(), "--all".to_string()],
                "Checking the ROMs of all systems".to_string(),
                None,
            ),
            _ => {
                return Ok(tool_error(
                    "pass system_id for one system, or all: true for every system",
                ));
            }
        };
        let outcome = self
            .run_action(
                label,
                Some("check_roms"),
                ctx.peer.clone(),
                ctx.meta.get_progress_token(),
                ctx.ct.clone(),
                move |connection, progress_bar| {
                    Box::pin(async move {
                        let matches = check_roms::subcommand().try_get_matches_from(argv)?;
                        check_roms::main(connection, &matches, progress_bar).await
                    })
                },
            )
            .await;
        if let Err(message) = outcome {
            return Ok(tool_error(message));
        }
        if let Some(system_id) = system_id {
            self.notify_resource_updated(&ctx.peer, &system_uri(system_id))
                .await;
        }
        Ok(ok_text(
            "check complete — mismatches were reported and trashed; see get_system for completion",
        ))
    }

    #[tool(
        description = "Convert one system's ROM files to a target format: ORIGINAL, 7Z, CHD, CSO, NSZ, RVZ, ZIP or ZSO (the PREFER_FORMAT values; arcade systems take only ORIGINAL or ZIP). Long jobs.",
        annotations(read_only_hint = false)
    )]
    async fn convert_roms(
        &self,
        Parameters(params): Parameters<ConvertRomsParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        if self.read_only {
            return Ok(read_only_denial("convert_roms"));
        }
        let system = match self.require_system(params.system_id).await? {
            Ok(system) => system,
            Err(result) => return Ok(result),
        };
        let format = params.format.to_uppercase();
        if !convert_roms::ALL_FORMATS.contains(&format.as_str()) {
            return Ok(tool_error(format!(
                "unknown format \"{format}\"; valid formats: {:?}",
                convert_roms::ALL_FORMATS
            )));
        }
        if system.arcade && !convert_roms::format_supported_for_system(&system, &format) {
            return Ok(tool_error(format!(
                "arcade systems only support {:?}",
                convert_roms::ARCADE_FORMATS
            )));
        }
        let argv = vec![
            "convert-roms".to_string(),
            "--system".to_string(),
            system.name.clone(),
            "--format".to_string(),
            format.clone(),
        ];
        let outcome = self
            .run_action(
                format!("Converting the ROMs of \"{}\" to {format}", system.name),
                Some("convert_roms"),
                ctx.peer.clone(),
                ctx.meta.get_progress_token(),
                ctx.ct.clone(),
                move |connection, progress_bar| {
                    Box::pin(async move {
                        let matches = convert_roms::subcommand().try_get_matches_from(argv)?;
                        convert_roms::main(connection, &matches, progress_bar).await
                    })
                },
            )
            .await;
        if let Err(message) = outcome {
            return Ok(tool_error(message));
        }
        self.notify_resource_updated(&ctx.peer, &system_uri(params.system_id))
            .await;
        Ok(ok_text(format!(
            "converted the ROMs of \"{}\" to {format}",
            system.name
        )))
    }

    #[tool(
        description = "Set or unset an oxyromon setting (same keys and values as `oxyromon config`). Omit value to unset a nullable setting. For list settings, value is the full desired comma-separated list — missing entries are added, extra ones removed.",
        annotations(read_only_hint = false, idempotent_hint = true)
    )]
    async fn set_setting(
        &self,
        Parameters(params): Parameters<SetSettingParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        if self.read_only {
            return Ok(read_only_denial("set_setting"));
        }
        let key = params.key.to_uppercase();
        let system_id = params.system_id;
        if let Some(system_id) = system_id
            && let Err(result) = self.require_system(system_id).await?
        {
            return Ok(result);
        }

        // Mirror the config subcommand's validation, but as tool errors the
        // caller can act on instead of warnings in a log line.
        match &params.value {
            None => {
                if !NULLABLES.contains(&key.as_str()) {
                    return Ok(tool_error(format!(
                        "\"{key}\" cannot be unset; list of unsettable keys: {NULLABLES:?}"
                    )));
                }
            }
            Some(value) => {
                if PATHS.contains(&key.as_str()) {
                    // config::set_setting canonicalizes and creates the directory
                } else if BOOLEANS.contains(&key.as_str()) {
                    if !matches!(value.as_str(), "true" | "false") {
                        return Ok(tool_error(format!("\"{key}\" takes \"true\" or \"false\"")));
                    }
                } else if let Some(options) = choice_options(&key) {
                    if !options.contains(&value.as_str()) {
                        return Ok(tool_error(format!(
                            "\"{value}\" is not a valid choice for \"{key}\"; valid: {options:?}"
                        )));
                    }
                    if key == "PREFER_FORMAT"
                        && let Some(system_id) = system_id
                    {
                        let connection = &mut self.pool.acquire().await.map_err(internal)?;
                        if let Some(system) = find_system_by_id_opt(connection, system_id).await
                            && !convert_roms::format_supported_for_system(&system, value)
                        {
                            return Ok(tool_error(format!(
                                "arcade systems only support {:?}",
                                convert_roms::ARCADE_FORMATS
                            )));
                        }
                    }
                } else if let Some(range) = integer_range(&key) {
                    match value.parse::<usize>() {
                        Ok(parsed) if range[0] <= parsed && parsed <= range[1] => (),
                        Ok(parsed) => {
                            return Ok(tool_error(format!(
                                "{parsed} is outside the valid range {:?} for \"{key}\"",
                                range
                            )));
                        }
                        Err(_) => {
                            return Ok(tool_error(format!(
                                "\"{key}\" takes an integer in {:?}, got \"{value}\"",
                                range
                            )));
                        }
                    }
                } else if !LISTS.contains(&key.as_str()) {
                    return Ok(tool_error(format!(
                        "unknown setting \"{key}\"; get_settings lists the keys in use"
                    )));
                }
            }
        }
        let closure_key = key.clone();
        let closure_value = params.value.clone();
        let outcome = self
            .run_action(
                format!("Setting \"{key}\""),
                None,
                ctx.peer.clone(),
                ctx.meta.get_progress_token(),
                ctx.ct.clone(),
                move |connection, progress_bar| {
                    Box::pin(async move {
                        match &closure_value {
                            Some(value) if LISTS.contains(&closure_key.as_str()) => {
                                // Replace the list wholesale: what the caller asked
                                // for minus what is there, and the other way.
                                let desired: Vec<String> = value
                                    .split(',')
                                    .map(|item| item.trim().to_string())
                                    .filter(|item| !item.is_empty())
                                    .collect();
                                let current =
                                    config::get_list(connection, &closure_key, system_id).await;
                                for item in &current {
                                    if !desired.contains(item) {
                                        config::remove_from_list(
                                            connection,
                                            progress_bar,
                                            &closure_key,
                                            item,
                                            system_id,
                                        )
                                        .await;
                                    }
                                }
                                for item in &desired {
                                    if !current.contains(item) {
                                        config::add_to_list(
                                            connection,
                                            progress_bar,
                                            &closure_key,
                                            item,
                                            system_id,
                                        )
                                        .await;
                                    }
                                }
                            }
                            Some(value) => {
                                config::set_setting(
                                    connection,
                                    progress_bar,
                                    &closure_key,
                                    value,
                                    system_id,
                                )
                                .await?
                            }
                            None => {
                                config::unset_setting(
                                    connection,
                                    progress_bar,
                                    &closure_key,
                                    system_id,
                                )
                                .await?
                            }
                        }
                        Ok(())
                    })
                },
            )
            .await;
        if let Err(message) = outcome {
            return Ok(tool_error(message));
        }
        Ok(match params.value {
            Some(value) => ok_text(format!("\"{key}\" is now \"{value}\"")),
            None => ok_text(format!("\"{key}\" was unset")),
        })
    }

    #[tool(
        description = "Forget ROM-file records whose files are gone (missing), records no ROM references (orphan), the Trash directories' files (trash), or files oxyromon does not know in the ROM directories (foreign). Any of trash/foreign deletes files: needs confirm: true.",
        annotations(read_only_hint = false, destructive_hint = true)
    )]
    async fn purge_roms(
        &self,
        Parameters(params): Parameters<PurgeRomsParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        if self.read_only {
            return Ok(read_only_denial("purge_roms"));
        }
        let deletes_files = params.trash || params.foreign;
        if deletes_files && !params.confirm {
            return Ok(need_confirm("purge_roms"));
        }
        let mut argv = vec!["purge-roms".to_string()];
        if params.missing {
            argv.push("--missing".to_string());
        }
        if params.orphan {
            argv.push("--orphan".to_string());
        }
        if params.trash {
            argv.push("--trash".to_string());
        }
        if params.foreign {
            argv.push("--foreign".to_string());
        }
        if argv.len() == 1 {
            return Ok(tool_error(
                "pass at least one of missing / orphan / trash / foreign",
            ));
        }
        argv.push("--yes".to_string());

        let outcome = self
            .run_action(
                "Purging ROM files".to_string(),
                Some("purge_roms"),
                ctx.peer.clone(),
                ctx.meta.get_progress_token(),
                ctx.ct.clone(),
                move |connection, progress_bar| {
                    Box::pin(async move {
                        let matches = purge_roms::subcommand().try_get_matches_from(argv)?;
                        purge_roms::main(connection, &matches, progress_bar).await
                    })
                },
            )
            .await;
        if let Err(message) = outcome {
            return Ok(tool_error(message));
        }
        self.notify_resources_changed(&ctx.peer).await;
        Ok(ok_text("purge complete"))
    }

    #[tool(
        description = "Purge one system: every imported file moves to its Trash directory, the system keeps its catalog but counts drop to zero. Needs confirm: true.",
        annotations(read_only_hint = false, destructive_hint = true)
    )]
    async fn purge_systems(
        &self,
        Parameters(params): Parameters<PurgeSystemParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        if self.read_only {
            return Ok(read_only_denial("purge_systems"));
        }
        if !params.confirm {
            return Ok(need_confirm("purge_systems"));
        }
        let system = match self.require_system(params.system_id).await? {
            Ok(system) => system,
            Err(result) => return Ok(result),
        };
        let name = system.name.clone();
        let outcome = self
            .run_action(
                format!("Purging \"{}\"", system.name),
                Some("purge"),
                ctx.peer.clone(),
                ctx.meta.get_progress_token(),
                ctx.ct.clone(),
                move |connection, progress_bar| {
                    Box::pin(async move { purge_system(connection, progress_bar, &system).await })
                },
            )
            .await;
        if let Err(message) = outcome {
            return Ok(tool_error(message));
        }
        self.notify_resource_updated(&ctx.peer, &system_uri(params.system_id))
            .await;
        Ok(ok_text(format!("purged \"{name}\"")))
    }

    #[tool(
        description = "Purge every IRD (JB folder) game of a PS3 system. Needs confirm: true.",
        annotations(read_only_hint = false, destructive_hint = true)
    )]
    async fn purge_irds(
        &self,
        Parameters(params): Parameters<PurgeIrdsParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        if self.read_only {
            return Ok(read_only_denial("purge_irds"));
        }
        if !params.confirm {
            return Ok(need_confirm("purge_irds"));
        }
        let system = match self.require_system(params.system_id).await? {
            Ok(system) => system,
            Err(result) => return Ok(result),
        };
        let argv = vec![
            "purge-irds".to_string(),
            "--system".to_string(),
            system.name.clone(),
        ];
        let outcome = self
            .run_action(
                format!("Purging the IRDs of \"{}\"", system.name),
                Some("purge_irds"),
                ctx.peer.clone(),
                ctx.meta.get_progress_token(),
                ctx.ct.clone(),
                move |connection, progress_bar| {
                    Box::pin(async move {
                        let matches = purge_irds::subcommand().try_get_matches_from(argv)?;
                        purge_irds::main(connection, &matches, progress_bar).await
                    })
                },
            )
            .await;
        if let Err(message) = outcome {
            return Ok(tool_error(message));
        }
        self.notify_resource_updated(&ctx.peer, &system_uri(params.system_id))
            .await;
        Ok(ok_text(format!("purged the IRDs of \"{}\"", system.name)))
    }
}

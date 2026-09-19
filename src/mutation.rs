use super::check_roms;
use super::config::{add_to_list, remove_from_list, set_bool, set_directory, set_string};
use super::convert_roms;
use super::database::*;
use super::download_dats::download_redump_system;
use super::generate_playlists;
use super::progress::*;
use super::purge_irds;
use super::purge_roms;
use super::purge_systems::purge_system;
use super::server::{SseMessage, sse_send};
use super::sort_roms;
use super::validator::*;
use async_graphql::{Context, Object, Result};
use futures::future::BoxFuture;
use serde_json::json;
use sqlx::SqliteConnection;
use sqlx::SqlitePool;
use tokio::sync::broadcast;

/// Spawn a background task that runs a CLI subcommand's `main()` and reports the
/// outcome over SSE as `{prefix}_started` / `{prefix}_complete` / `{prefix}_error`.
///
/// The connection is acquired *inside* the task and a failure to acquire is
/// reported as an SSE error rather than a panic, so the UI never hangs waiting
/// for a completion event that will never arrive.
fn spawn_cli_action(
    pool: SqlitePool,
    sse_tx: broadcast::Sender<SseMessage>,
    prefix: &'static str,
    start_message: String,
    complete_message: String,
    fail_prefix: String,
    run: impl for<'a> FnOnce(&'a mut SqliteConnection) -> BoxFuture<'a, anyhow::Result<()>>
    + Send
    + 'static,
) {
    tokio::spawn(async move {
        let mut connection = match pool.acquire().await {
            Ok(connection) => connection,
            Err(e) => {
                let message = format!("{fail_prefix}: {e}");
                sse_send(
                    &sse_tx,
                    &format!("{prefix}_error"),
                    json!({ "success": false, "message": message }),
                );
                log::error!("{message}");
                return;
            }
        };
        sse_send(
            &sse_tx,
            &format!("{prefix}_started"),
            json!({ "message": start_message }),
        );
        match run(&mut connection).await {
            Ok(()) => {
                log::info!("{complete_message}");
                sse_send(
                    &sse_tx,
                    &format!("{prefix}_complete"),
                    json!({ "success": true, "message": complete_message }),
                );
            }
            Err(e) => {
                let message = format!("{fail_prefix}: {e:#}");
                sse_send(
                    &sse_tx,
                    &format!("{prefix}_error"),
                    json!({ "success": false, "message": message }),
                );
                log::error!("{message}");
            }
        }
    });
}

/// The typed error returned by every mutation that resolves a caller-supplied
/// `system_id` before doing work.
fn system_not_found(system_id: i64) -> async_graphql::Error {
    async_graphql::Error::new(format!("System {system_id} not found"))
}

/// Acquire a pooled connection for a request-path mutation, turning a pool
/// failure into a typed GraphQL error instead of a panic. The spawned actions
/// hold their connections for the whole run, so under concurrency a request-path
/// acquire can genuinely fail; that must surface as an error, not a 500.
async fn acquire_connection(pool: &SqlitePool) -> Result<sqlx::pool::PoolConnection<sqlx::Sqlite>> {
    pool.acquire().await.map_err(|e| {
        async_graphql::Error::new(format!("Failed to acquire database connection: {e}"))
    })
}

pub struct Mutation;

#[Object]
impl Mutation {
    async fn add_to_list(
        &self,
        ctx: &Context<'_>,
        key: String,
        value: String,
        system_id: Option<i64>,
    ) -> Result<bool> {
        log::debug!("mutation::add_to_list({}, {})", key, value);
        let pool = ctx.data_unchecked::<SqlitePool>();
        let progress_bar = get_progress_bar(0, get_none_progress_style());
        let mut connection = acquire_connection(pool).await?;
        add_to_list(&mut connection, &progress_bar, &key, &value, system_id).await;
        Ok(true)
    }

    async fn remove_from_list(
        &self,
        ctx: &Context<'_>,
        key: String,
        value: String,
        system_id: Option<i64>,
    ) -> Result<bool> {
        log::debug!("mutation::remove_to_list({}, {})", key, value);
        let pool = ctx.data_unchecked::<SqlitePool>();
        let progress_bar = get_progress_bar(0, get_none_progress_style());
        let mut connection = acquire_connection(pool).await?;
        remove_from_list(&mut connection, &progress_bar, &key, &value, system_id).await;
        Ok(true)
    }

    async fn set_bool(
        &self,
        ctx: &Context<'_>,
        key: String,
        value: bool,
        system_id: Option<i64>,
    ) -> Result<bool> {
        log::debug!("mutation::set_bool({}, {})", key, value);
        let pool = ctx.data_unchecked::<SqlitePool>();
        let mut connection = acquire_connection(pool).await?;
        set_bool(&mut connection, &key, value, system_id).await;
        Ok(true)
    }

    async fn set_prefer_regions(
        &self,
        ctx: &Context<'_>,
        #[graphql(validator(custom = "PreferRegionValidator::new()"))] value: String,
        system_id: Option<i64>,
    ) -> Result<bool> {
        log::debug!("mutation::set_prefer_regions({})", value);
        let pool = ctx.data_unchecked::<SqlitePool>();
        let mut connection = acquire_connection(pool).await?;
        set_string(&mut connection, "PREFER_REGIONS", &value, system_id).await;
        Ok(true)
    }

    async fn set_prefer_versions(
        &self,
        ctx: &Context<'_>,
        #[graphql(validator(custom = "PreferVersionValidator::new()"))] value: String,
        system_id: Option<i64>,
    ) -> Result<bool> {
        log::debug!("mutation::set_prefer_versions({})", value);
        let pool = ctx.data_unchecked::<SqlitePool>();
        let mut connection = acquire_connection(pool).await?;
        set_string(&mut connection, "PREFER_VERSIONS", &value, system_id).await;
        Ok(true)
    }

    async fn set_subfolder_scheme(
        &self,
        ctx: &Context<'_>,
        key: String,
        #[graphql(validator(custom = "SubfolderSchemeValidator::new()"))] value: String,
        system_id: Option<i64>,
    ) -> Result<bool> {
        log::debug!("mutation::set_subfolder_scheme({}, {})", key, value);
        let pool = ctx.data_unchecked::<SqlitePool>();
        let mut connection = acquire_connection(pool).await?;
        set_string(&mut connection, &key, &value, system_id).await;
        Ok(true)
    }

    async fn set_directory(
        &self,
        ctx: &Context<'_>,
        key: String,
        #[graphql(validator(custom = "DirectoryValidator::new()"))] value: String,
        system_id: Option<i64>,
    ) -> Result<bool> {
        log::debug!("mutation::set_directory({}, {})", key, value);
        let pool = ctx.data_unchecked::<SqlitePool>();
        let mut connection = acquire_connection(pool).await?;
        set_directory(&mut connection, &key, &value, system_id).await;
        Ok(true)
    }

    /// Fetch the named Redump systems' DAT files and import them.
    ///
    /// Returns as soon as the work is handed to a background task; progress and
    /// the outcome arrive over SSE, the same way importing an uploaded DAT does.
    async fn download_dats(&self, ctx: &Context<'_>, systems: Vec<String>) -> Result<bool> {
        log::debug!("mutation::download_dats({:?})", systems);
        let pool = ctx.data_unchecked::<SqlitePool>().clone();
        let sse_tx = ctx
            .data_unchecked::<broadcast::Sender<SseMessage>>()
            .clone();

        tokio::spawn(async move {
            let mut connection = match pool.acquire().await {
                Ok(connection) => connection,
                Err(e) => {
                    sse_send(
                        &sse_tx,
                        "download_dats_error",
                        json!({
                            "success": false,
                            "message": format!("Failed to download DAT files: {e}"),
                        }),
                    );
                    log::error!("Failed to acquire connection to download DAT files: {e}");
                    return;
                }
            };
            let progress_bar = ProgressBar::hidden();
            let total = systems.len();

            sse_send(
                &sse_tx,
                "download_dats_started",
                json!({
                    "total": total,
                    "message": format!("Downloading {} DAT file(s)", total),
                }),
            );

            // Deliberately quiet between the two: a per-system event would mean
            // a toast each, and this list can run to the whole catalogue.
            let mut failed: Vec<String> = Vec::new();
            for system_name in systems.iter() {
                if let Err(e) =
                    download_redump_system(&mut connection, &progress_bar, system_name, false).await
                {
                    log::error!("Failed to download {}: {:#}", system_name, e);
                    failed.push(system_name.clone());
                }
            }

            if failed.is_empty() {
                sse_send(
                    &sse_tx,
                    "download_dats_complete",
                    json!({
                        "total": total,
                        "success": true,
                        "message": format!("Downloaded {} DAT file(s)", total),
                    }),
                );
            } else {
                sse_send(
                    &sse_tx,
                    "download_dats_error",
                    json!({
                        "success": false,
                        "failed": failed,
                        "message": format!("Failed to download: {}", failed.join(", ")),
                    }),
                );
            }
        });

        Ok(true)
    }

    async fn purge_system(&self, ctx: &Context<'_>, system_id: i64) -> Result<bool> {
        log::debug!("mutation::purge_system({})", system_id);
        let pool = ctx.data_unchecked::<SqlitePool>().clone();
        let sse_tx = ctx
            .data_unchecked::<broadcast::Sender<SseMessage>>()
            .clone();
        let mut connection = acquire_connection(&pool).await?;

        let system = match find_system_by_id_opt(&mut connection, system_id).await {
            Some(system) => system,
            None => return Err(system_not_found(system_id)),
        };
        let system_name = system.name.clone();

        // Spawn background task for deletion
        tokio::spawn(async move {
            let mut connection = match pool.acquire().await {
                Ok(connection) => connection,
                Err(e) => {
                    sse_send(
                        &sse_tx,
                        "purge_error",
                        json!({
                            "system_id": system_id,
                            "system_name": system_name,
                            "success": false,
                            "error": format!("{e}"),
                            "message": format!("Failed to delete system '{}': {e}", system_name)
                        }),
                    );
                    log::error!("Failed to acquire connection to purge system {system_id}: {e}");
                    return;
                }
            };
            let progress_bar = ProgressBar::hidden();

            // Send start notification
            sse_send(
                &sse_tx,
                "purge_started",
                json!({
                    "system_id": system_id,
                    "system_name": system_name,
                    "message": format!("Starting deletion of system '{}'", system_name)
                }),
            );

            // Perform the actual deletion
            match purge_system(&mut connection, &progress_bar, &system).await {
                Ok(_) => {
                    sse_send(
                        &sse_tx,
                        "purge_complete",
                        json!({
                            "system_id": system_id,
                            "system_name": system_name,
                            "success": true,
                            "message": format!("System '{}' has been successfully deleted", system_name)
                        }),
                    );
                    log::info!("Successfully purged system: {}", system_name);
                }
                Err(e) => {
                    sse_send(
                        &sse_tx,
                        "purge_error",
                        json!({
                            "system_id": system_id,
                            "system_name": system_name,
                            "success": false,
                            "error": format!("{:#}", e),
                            "message": format!("Failed to delete system '{}': {:#}", system_name, e)
                        }),
                    );
                    log::error!("Failed to purge system {}: {:#}", system_name, e);
                }
            }
        });

        Ok(true)
    }

    /// Sort the ROMs of the given system, or of every system when none is
    /// given, according to the region and version preferences.
    ///
    /// Returns as soon as the work is handed to a background task; the outcome
    /// arrives over SSE, the same way purging a system does.
    async fn sort_roms(&self, ctx: &Context<'_>, system_id: Option<i64>) -> Result<bool> {
        log::debug!("mutation::sort_roms({:?})", system_id);
        let pool = ctx.data_unchecked::<SqlitePool>().clone();
        let sse_tx = ctx
            .data_unchecked::<broadcast::Sender<SseMessage>>()
            .clone();
        let mut connection = acquire_connection(&pool).await?;

        let system_name = match system_id {
            Some(system_id) => match find_system_by_id_opt(&mut connection, system_id).await {
                Some(system) => Some(system.name),
                None => return Err(system_not_found(system_id)),
            },
            None => None,
        };

        let message = match &system_name {
            Some(name) => format!("Sorting the ROMs of '{}'", name),
            None => "Sorting the ROMs of all systems".to_string(),
        };
        let complete_message = match &system_name {
            Some(name) => format!("Sorted the ROMs of '{}'", name),
            None => "Sorted the ROMs of all systems".to_string(),
        };

        spawn_cli_action(
            pool,
            sse_tx,
            "sort_roms",
            message,
            complete_message,
            "Failed to sort ROMs".to_string(),
            move |connection| {
                Box::pin(async move {
                    let progress_bar = ProgressBar::hidden();
                    let mut arguments: Vec<String> = vec!["sort-roms".to_string()];
                    match system_name {
                        Some(name) => {
                            arguments.push("-s".to_string());
                            arguments.push(name);
                        }
                        None => arguments.push("-a".to_string()),
                    }
                    // The action was asked for from the UI, so answer the
                    // per-system confirmation instead of waiting on a TTY.
                    arguments.push("-y".to_string());
                    let matches = sort_roms::subcommand().get_matches_from(arguments);
                    sort_roms::main(connection, &matches, &progress_bar).await
                })
            },
        );

        Ok(true)
    }

    /// Check the integrity of the ROMs of the given system, or of every system
    /// when none is given, by re-hashing them; corrupt files are moved to the
    /// Trash directory.
    async fn check_roms(&self, ctx: &Context<'_>, system_id: Option<i64>) -> Result<bool> {
        log::debug!("mutation::check_roms({:?})", system_id);
        let pool = ctx.data_unchecked::<SqlitePool>().clone();
        let sse_tx = ctx
            .data_unchecked::<broadcast::Sender<SseMessage>>()
            .clone();
        let mut connection = acquire_connection(&pool).await?;

        let system_name = match system_id {
            Some(system_id) => match find_system_by_id_opt(&mut connection, system_id).await {
                Some(system) => Some(system.name),
                None => return Err(system_not_found(system_id)),
            },
            None => None,
        };

        let message = match &system_name {
            Some(name) => format!("Checking the ROMs of '{}'", name),
            None => "Checking the ROMs of all systems".to_string(),
        };
        let complete_message = match &system_name {
            Some(name) => format!("Checked the ROMs of '{}'", name),
            None => "Checked the ROMs of all systems".to_string(),
        };

        spawn_cli_action(
            pool,
            sse_tx,
            "check_roms",
            message,
            complete_message,
            "Failed to check ROMs".to_string(),
            move |connection| {
                Box::pin(async move {
                    let progress_bar = ProgressBar::hidden();
                    let mut arguments: Vec<String> = vec!["check-roms".to_string()];
                    match system_name {
                        Some(name) => {
                            arguments.push("--system".to_string());
                            arguments.push(name);
                        }
                        None => arguments.push("-a".to_string()),
                    }
                    let matches = check_roms::subcommand().get_matches_from(arguments);
                    check_roms::main(connection, &matches, &progress_bar).await
                })
            },
        );

        Ok(true)
    }

    /// Generate M3U playlists for every multi-disc game, across all systems.
    async fn generate_playlists(&self, ctx: &Context<'_>) -> Result<bool> {
        log::debug!("mutation::generate_playlists()");
        let pool = ctx.data_unchecked::<SqlitePool>().clone();
        let sse_tx = ctx
            .data_unchecked::<broadcast::Sender<SseMessage>>()
            .clone();

        spawn_cli_action(
            pool,
            sse_tx,
            "generate_playlists",
            "Generating playlists for all systems".to_string(),
            "Generated playlists for all systems".to_string(),
            "Failed to generate playlists".to_string(),
            |connection| {
                Box::pin(async move {
                    let progress_bar = ProgressBar::hidden();
                    let arguments = vec!["generate-playlists".to_string(), "-a".to_string()];
                    let matches = generate_playlists::subcommand().get_matches_from(arguments);
                    generate_playlists::main(connection, &matches, &progress_bar).await
                })
            },
        );

        Ok(true)
    }

    /// Purge the selected categories of ROM files: missing, orphan, trashed,
    /// and foreign.
    async fn purge_roms(
        &self,
        ctx: &Context<'_>,
        missing: bool,
        orphan: bool,
        trash: bool,
        foreign: bool,
    ) -> Result<bool> {
        log::debug!(
            "mutation::purge_roms({}, {}, {}, {})",
            missing,
            orphan,
            trash,
            foreign
        );
        let pool = ctx.data_unchecked::<SqlitePool>().clone();
        let sse_tx = ctx
            .data_unchecked::<broadcast::Sender<SseMessage>>()
            .clone();

        spawn_cli_action(
            pool,
            sse_tx,
            "purge_roms",
            "Purging ROM files".to_string(),
            "Purged ROM files".to_string(),
            "Failed to purge ROM files".to_string(),
            move |connection| {
                Box::pin(async move {
                    let progress_bar = ProgressBar::hidden();
                    let mut arguments: Vec<String> = vec!["purge-roms".to_string()];
                    if missing {
                        arguments.push("-m".to_string());
                    }
                    if orphan {
                        arguments.push("-o".to_string());
                    }
                    if trash {
                        arguments.push("-t".to_string());
                    }
                    if foreign {
                        arguments.push("-f".to_string());
                    }
                    arguments.push("-y".to_string());
                    let matches = purge_roms::subcommand().get_matches_from(arguments);
                    purge_roms::main(connection, &matches, &progress_bar).await
                })
            },
        );

        Ok(true)
    }

    /// Convert the ROM files of the given system to the given format.
    async fn convert_roms(
        &self,
        ctx: &Context<'_>,
        system_id: i64,
        format: String,
    ) -> Result<bool> {
        log::debug!("mutation::convert_roms({}, {:?})", system_id, format);
        let pool = ctx.data_unchecked::<SqlitePool>().clone();
        let sse_tx = ctx
            .data_unchecked::<broadcast::Sender<SseMessage>>()
            .clone();
        let mut connection = acquire_connection(&pool).await?;

        let system_name = match find_system_by_id_opt(&mut connection, system_id).await {
            Some(system) => system.name,
            None => return Err(system_not_found(system_id)),
        };
        if !convert_roms::ALL_FORMATS.contains(&format.as_str()) {
            return Err(async_graphql::Error::new(format!(
                "Unsupported format '{}'; expected one of {}",
                format,
                convert_roms::ALL_FORMATS.join(", ")
            )));
        }

        let message = format!("Converting the ROMs of '{}' to {}", system_name, format);
        let complete_message = format!("Converted the ROMs of '{}' to {}", system_name, format);

        spawn_cli_action(
            pool,
            sse_tx,
            "convert_roms",
            message,
            complete_message,
            "Failed to convert ROMs".to_string(),
            move |connection| {
                Box::pin(async move {
                    let progress_bar = ProgressBar::hidden();
                    let arguments: Vec<String> = vec![
                        "convert-roms".to_string(),
                        "-s".to_string(),
                        system_name,
                        "-f".to_string(),
                        format,
                    ];
                    let matches = convert_roms::subcommand().get_matches_from(arguments);
                    convert_roms::main(connection, &matches, &progress_bar).await
                })
            },
        );

        Ok(true)
    }

    /// Purge every IRD (JB folder) game of one system.
    async fn purge_irds(&self, ctx: &Context<'_>, system_id: i64) -> Result<bool> {
        log::debug!("mutation::purge_irds({})", system_id);
        let pool = ctx.data_unchecked::<SqlitePool>().clone();
        let sse_tx = ctx
            .data_unchecked::<broadcast::Sender<SseMessage>>()
            .clone();
        let mut connection = acquire_connection(&pool).await?;
        let system_name = match find_system_by_id_opt(&mut connection, system_id).await {
            Some(system) => system.name,
            None => return Err(system_not_found(system_id)),
        };

        let message = format!("Purging the IRDs of '{}'", system_name);
        let complete_message = format!("Purged the IRDs of '{}'", system_name);

        spawn_cli_action(
            pool,
            sse_tx,
            "purge_irds",
            message,
            complete_message,
            "Failed to purge IRDs".to_string(),
            move |connection| {
                Box::pin(async move {
                    let progress_bar = ProgressBar::hidden();
                    let arguments = vec![
                        "purge-irds".to_string(),
                        "--system".to_string(),
                        system_name,
                    ];
                    let matches = purge_irds::subcommand().get_matches_from(arguments);
                    purge_irds::main(connection, &matches, &progress_bar).await
                })
            },
        );

        Ok(true)
    }
}

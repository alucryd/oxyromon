use super::config::{add_to_list, remove_from_list, set_bool, set_directory, set_string};
use super::database::*;
use super::download_dats::download_redump_system;
use super::progress::*;
use super::check_roms;
use super::purge_systems::purge_system;
use super::server::{SseMessage, sse_send};
use super::sort_roms;
use super::validator::*;
use async_graphql::{Context, Object, Result};
use serde_json::json;
use sqlx::SqlitePool;
use tokio::sync::broadcast;

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
        add_to_list(
            &mut pool.acquire().await.unwrap(),
            &progress_bar,
            &key,
            &value,
            system_id,
        )
        .await;
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
        remove_from_list(
            &mut pool.acquire().await.unwrap(),
            &progress_bar,
            &key,
            &value,
            system_id,
        )
        .await;
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
        set_bool(&mut pool.acquire().await.unwrap(), &key, value, system_id).await;
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
        set_string(
            &mut pool.acquire().await.unwrap(),
            "PREFER_REGIONS",
            &value,
            system_id,
        )
        .await;
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
        set_string(
            &mut pool.acquire().await.unwrap(),
            "PREFER_VERSIONS",
            &value,
            system_id,
        )
        .await;
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
        set_string(&mut pool.acquire().await.unwrap(), &key, &value, system_id).await;
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
        set_directory(&mut pool.acquire().await.unwrap(), &key, &value, system_id).await;
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
            let mut connection = pool.acquire().await.unwrap();
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
        let mut connection = pool.acquire().await.unwrap();

        let system = find_system_by_id(&mut connection, system_id).await;
        let system_name = system.name.clone();

        // Spawn background task for deletion
        tokio::spawn(async move {
            let mut connection = pool.acquire().await.unwrap();
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
        let mut connection = pool.acquire().await.unwrap();

        let system_name = match system_id {
            Some(system_id) => {
                let system = find_system_by_id(&mut connection, system_id).await;
                Some(system.name)
            }
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

        tokio::spawn(async move {
            let mut connection = pool.acquire().await.unwrap();
            let progress_bar = ProgressBar::hidden();

            sse_send(&sse_tx, "sort_roms_started", json!({ "message": message }));

            let mut arguments: Vec<String> = vec!["sort-roms".to_string()];
            match system_name {
                Some(name) => {
                    arguments.push("-s".to_string());
                    arguments.push(name);
                }
                None => arguments.push("-a".to_string()),
            }
            // The action was asked for from the UI, so answer the per-system
            // confirmation instead of waiting on a TTY that is not there.
            arguments.push("-y".to_string());

            let matches = sort_roms::subcommand().get_matches_from(arguments);
            match sort_roms::main(&mut connection, &matches, &progress_bar).await {
                Ok(_) => {
                    log::info!("Successfully sorted ROMs: {}", complete_message);
                    sse_send(
                        &sse_tx,
                        "sort_roms_complete",
                        json!({ "success": true, "message": complete_message }),
                    );
                }
                Err(e) => {
                    sse_send(
                        &sse_tx,
                        "sort_roms_error",
                        json!({ "success": false, "message": format!("Failed to sort ROMs: {:#}", e) }),
                    );
                    log::error!("Failed to sort ROMs: {:#}", e);
                }
            }
        });

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
        let mut connection = pool.acquire().await.unwrap();

        let system_name = match system_id {
            Some(system_id) => {
                let system = find_system_by_id(&mut connection, system_id).await;
                Some(system.name)
            }
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

        tokio::spawn(async move {
            let mut connection = pool.acquire().await.unwrap();
            let progress_bar = ProgressBar::hidden();

            sse_send(&sse_tx, "check_roms_started", json!({ "message": message }));

            let mut arguments: Vec<String> = vec!["check-roms".to_string()];
            match system_name {
                Some(name) => {
                    arguments.push("--system".to_string());
                    arguments.push(name);
                }
                None => arguments.push("-a".to_string()),
            }

            let matches = check_roms::subcommand().get_matches_from(arguments);
            match check_roms::main(&mut connection, &matches, &progress_bar).await {
                Ok(_) => {
                    log::info!("Successfully checked ROMs: {}", complete_message);
                    sse_send(
                        &sse_tx,
                        "check_roms_complete",
                        json!({ "success": true, "message": complete_message }),
                    );
                }
                Err(e) => {
                    sse_send(
                        &sse_tx,
                        "check_roms_error",
                        json!({ "success": false, "message": format!("Failed to check ROMs: {:#}", e) }),
                    );
                    log::error!("Failed to check ROMs: {:#}", e);
                }
            }
        });

        Ok(true)
    }
}

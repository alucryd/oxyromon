// Prompts: two guided workflows. They return a real snapshot of the library
// (not a generic template), so the model can start acting immediately.

use super::output::SystemOut;
use super::params::{AuditSystemArgs, OnboardSystemArgs};
use super::{OxyromonMcp, internal};
use rmcp::ErrorData as McpError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{ContentBlock, GetPromptResult, PromptMessage, Role};
use rmcp::{prompt, prompt_router};

#[prompt_router(vis = "pub(crate)")]
impl OxyromonMcp {
    #[prompt(
        description = "Guided flow to add a system to the library: fetch or locate its DATs, import them, import the ROM files, verify, and sort."
    )]
    async fn onboard_system(
        &self,
        Parameters(args): Parameters<OnboardSystemArgs>,
    ) -> Result<GetPromptResult, McpError> {
        let connection = &mut self.pool.acquire().await.map_err(internal)?;
        let matches = crate::database::find_systems_by_name_like(connection, &args.system).await;
        let message = if matches.is_empty() {
            let downloadable = crate::download_dats::find_redump_systems(connection, false).await;
            let is_redump = downloadable
                .iter()
                .any(|name| name.to_lowercase().contains(&args.system.to_lowercase()));
            let first_step = if is_redump {
                format!(
                    "1. This is a Redump system — call `download_dats` with the exact name \
                     from `downloadable_systems` matching \"{system}\", then `import_dats` \
                     is not needed for it separately (download_dats feeds the import).",
                    system = args.system
                )
            } else {
                "1. This system is not in the library and not on Redump. Locate its Logiqx \
                     DAT files (No-Intro dat directory for consoles, MAME for arcade) on disk \
                     and call `import_dats` with their paths (a ZIP of DATs works too)."
                    .to_string()
            };
            format!(
                "Help me onboard the system \"{system}\" into oxyromon.\n\n\
                 It is not imported yet.\n\n\
                 {first_step}\n\
                 2. Check the DATs landed: `list_systems` should show it with its game catalog.\n\
                 3. Ask me where my ROM files for it live, then call `import_roms` with those \
                     paths and the system_id — ambiguous matches auto-resolve to the first.\n\
                 4. Verify with `check_roms` (system_id) and fix or trash mismatches it reports.\n\
                 5. File everything with `sort_roms` (system_id) so region folders and the 1G1R \
                     selection follow my settings; review the Trash results before purging.\n\n\
                 Report completion from `get_system` after each step.",
                system = args.system
            )
        } else {
            let systems: Vec<SystemOut> = matches.iter().map(SystemOut::from_system).collect();
            let snapshot = serde_json::to_string_pretty(&systems)
                .unwrap_or_else(|e| format!("could not serialize: {e}"));
            format!(
                "Help me onboard the system \"{system}\" into oxyromon.\n\n\
                 It already exists —\n{snapshot}\n\n\
                 1. If its catalog is stale, refresh it with `import_dats` (update: true).\n\
                 2. Ask me where newer ROM files for it live and `import_roms` them with its \
                     system_id.\n\
                 3. `check_roms` (system_id) to re-verify everything on disk.\n\
                 4. `sort_roms` (system_id) to re-file the library per my settings.\n\n\
                 Use `get_system` to report completion before and after.",
                system = args.system
            )
        };
        Ok(GetPromptResult::new(vec![PromptMessage::new(
            Role::User,
            ContentBlock::text(message),
        )]))
    }

    #[prompt(
        description = "Audit one system: a snapshot of completion, sizes, and per-game state, plus the fix-it workflow to walk through."
    )]
    async fn audit_system(
        &self,
        Parameters(args): Parameters<AuditSystemArgs>,
    ) -> Result<GetPromptResult, McpError> {
        let connection = &mut self.pool.acquire().await.map_err(internal)?;
        let Some(system) = crate::database::find_system_by_id_opt(connection, args.system_id).await
        else {
            return Err(internal(format!(
                "system {} not found; list_systems has the ids",
                args.system_id
            )));
        };
        let snapshot = serde_json::json!({
            "system": SystemOut::from_system(&system),
        });
        let message = format!(
            "Audit the oxyromon system \"{name}\".\n\nCurrent snapshot:\n{snapshot:#}\n\n\
             Walk this workflow, reporting between steps:\n\
             1. `check_roms` (system_id {id}) to re-hash every imported file against the \
             catalog; mismatches get trashed — review the list before accepting.\n\
             2. `get_system` (system_id {id}) afterwards: compare completion and sizes, note \
             expected-vs-actual size gaps that hint at wrong-format files.\n\
             3. List incomplete games with `list_games` (system_id {id}) and dig into a few with \
             `get_game` to characterize what is missing (which disks, which region).\n\
             4. If files sit in Trash or orphans accumulated, propose the matching \
             `purge_roms` flags — they need confirm: true, so ask me first.\n\
             5. Finish with `sort_roms` (system_id {id}) if anything was added or removed.\n\n\
             End with a short summary: completion before/after, size on disk, what remains \
             missing and why.",
            name = system.name,
            id = system.id,
        );
        Ok(GetPromptResult::new(vec![PromptMessage::new(
            Role::User,
            ContentBlock::text(message),
        )]))
    }
}

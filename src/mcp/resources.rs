// Resources: stable read-only views a client can mount as context.
//
//   oxyromon://status              the status payload
//   oxyromon://systems             the system index (id + name)
//   oxyromon://systems/{id}        one system: stats, settings, game counts
//   oxyromon://settings            the global settings

use super::OxyromonMcp;
use super::output::{SettingOut, SystemOut};
use super::read::build_status;
use rmcp::ErrorData as McpError;
use rmcp::model::{
    ListResourceTemplatesResult, ListResourcesResult, PaginatedRequestParams,
    ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Resource,
    ResourceContents, ResourceTemplate,
};
use rmcp::service::{RequestContext, RoleServer};

pub(crate) fn system_uri(system_id: i64) -> String {
    format!("oxyromon://systems/{system_id}")
}

pub(crate) async fn list_resources(
    server: &OxyromonMcp,
    _request: Option<PaginatedRequestParams>,
    _context: RequestContext<RoleServer>,
) -> Result<ListResourcesResult, McpError> {
    let connection = &mut server.pool.acquire().await.map_err(internal)?;
    let mut resources = vec![
        Resource::new("oxyromon://status", "oxyROMon status")
            .with_description("Version, directories, library counts, dependency availability."),
        Resource::new("oxyromon://systems", "System index")
            .with_description("Id and name of every system in the library."),
        Resource::new("oxyromon://settings", "Settings")
            .with_description("Global settings with their current values."),
    ];
    for system in crate::database::find_systems(connection).await {
        resources.push(
            Resource::new(system_uri(system.id), format!("System: {}", system.name))
                .with_mime_type("application/json"),
        );
    }
    Ok(ListResourcesResult {
        resources,
        ..Default::default()
    })
}

pub(crate) async fn list_resource_templates(
    _server: &OxyromonMcp,
    _request: Option<PaginatedRequestParams>,
    _context: RequestContext<RoleServer>,
) -> Result<ListResourceTemplatesResult, McpError> {
    Ok(ListResourceTemplatesResult {
        resource_templates: vec![
            ResourceTemplate::new("oxyromon://systems/{system_id}", "System")
                .with_description("One system: stats, settings, game counts (JSON)."),
        ],
        ..Default::default()
    })
}

pub(crate) async fn read_resource(
    server: &OxyromonMcp,
    request: ReadResourceRequestParams,
    _context: RequestContext<RoleServer>,
) -> Result<ReadResourceResponse, McpError> {
    let uri = request.uri.as_str();
    let contents = if uri == "oxyromon://status" {
        let connection = &mut server.pool.acquire().await.map_err(internal)?;
        let status = build_status(connection, server.read_only).await;
        (
            uri.to_string(),
            serde_json::to_string_pretty(&status).map_err(|e| internal(e.to_string()))?,
        )
    } else if uri == "oxyromon://systems" {
        let connection = &mut server.pool.acquire().await.map_err(internal)?;
        let systems: Vec<serde_json::Value> = crate::database::find_systems(connection)
            .await
            .iter()
            .map(|system| serde_json::json!({ "id": system.id, "name": system.name }))
            .collect();
        (
            uri.to_string(),
            serde_json::to_string_pretty(&systems).map_err(|e| internal(e.to_string()))?,
        )
    } else if uri == "oxyromon://settings" {
        let connection = &mut server.pool.acquire().await.map_err(internal)?;
        let settings: Vec<SettingOut> = crate::database::find_settings(connection, None)
            .await
            .into_iter()
            .map(|setting| SettingOut {
                key: setting.key,
                value: setting.value,
                system_id: setting.system_id,
            })
            .collect();
        (
            uri.to_string(),
            serde_json::to_string_pretty(&settings).map_err(|e| internal(e.to_string()))?,
        )
    } else if let Some(id) = uri.strip_prefix("oxyromon://systems/") {
        let Ok(system_id) = id.parse::<i64>() else {
            return Err(McpError::resource_not_found(
                format!("unknown system id in \"{uri}\""),
                None,
            ));
        };
        let connection = &mut server.pool.acquire().await.map_err(internal)?;
        let Some(system) = crate::database::find_system_by_id_opt(connection, system_id).await
        else {
            return Err(McpError::resource_not_found(
                format!("system {system_id} not found"),
                None,
            ));
        };
        let payload = serde_json::json!({
            "system": SystemOut::from_system(&system),
            "settings": crate::database::find_settings(connection, Some(system_id))
                .await
                .into_iter()
                .map(|setting| serde_json::json!({
                    "key": setting.key,
                    "value": setting.value,
                }))
                .collect::<Vec<_>>(),
        });
        (
            uri.to_string(),
            serde_json::to_string_pretty(&payload).map_err(|e| internal(e.to_string()))?,
        )
    } else {
        return Err(McpError::resource_not_found(
            format!("unknown resource \"{uri}\""),
            None,
        ));
    };
    Ok(ReadResourceResponse::from(ReadResourceResult::new(vec![
        ResourceContents::text(contents.1, contents.0).with_mime_type("application/json"),
    ])))
}

fn internal(message: impl std::fmt::Display) -> McpError {
    McpError::internal_error(std::borrow::Cow::Owned(message.to_string()), None)
}

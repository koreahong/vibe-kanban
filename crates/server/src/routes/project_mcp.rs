// QRAFT-CUSTOM: Project-level MCP config (.mcp.json) endpoint
// Reads and writes .mcp.json in the workspace's container_ref directory.
// Claude Code CLI reads this file automatically alongside ~/.claude.json.

use std::collections::HashMap;

use axum::{
    Json, Router,
    extract::{Query, State},
    response::Json as ResponseJson,
    routing::get,
};
use db::models::workspace::Workspace;
use executors::mcp_config::{McpConfig, read_agent_config, write_agent_config};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::fs;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};
use deployment::Deployment;

#[derive(Debug, Deserialize)]
pub struct ProjectMcpQuery {
    pub workspace_id: Uuid,
}

#[derive(Debug, Serialize)]
pub struct ProjectMcpResponse {
    pub servers: HashMap<String, Value>,
    pub config_path: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProjectMcpBody {
    pub servers: HashMap<String, Value>,
}

fn project_mcp_config() -> McpConfig {
    McpConfig::new(
        vec!["mcpServers".to_string()],
        serde_json::json!({ "mcpServers": {} }),
        serde_json::json!({}),
        false,
    )
}

async fn get_project_mcp(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ProjectMcpQuery>,
) -> Result<ResponseJson<ProjectMcpResponse>, ApiError> {
    let pool = &deployment.db().pool;
    let workspace = Workspace::find_by_id(pool, query.workspace_id)
        .await?
        .ok_or_else(|| ApiError::BadRequest("Workspace not found".to_string()))?;

    let container_ref = workspace.container_ref.unwrap_or_default();
    let mcp_path = std::path::PathBuf::from(&container_ref).join(".mcp.json");

    if !mcp_path.exists() {
        return Ok(ResponseJson(ProjectMcpResponse {
            servers: HashMap::new(),
            config_path: mcp_path.to_string_lossy().to_string(),
        }));
    }

    let mcpc = project_mcp_config();
    let raw = read_agent_config(&mcp_path, &mcpc)
        .await
        .map_err(|e| ApiError::BadRequest(format!("Failed to read .mcp.json: {}", e)))?;

    let servers = match raw.get("mcpServers").and_then(|v| v.as_object()) {
        Some(obj) => obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        None => HashMap::new(),
    };

    Ok(ResponseJson(ProjectMcpResponse {
        servers,
        config_path: mcp_path.to_string_lossy().to_string(),
    }))
}

async fn update_project_mcp(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ProjectMcpQuery>,
    Json(body): Json<UpdateProjectMcpBody>,
) -> Result<ResponseJson<ProjectMcpResponse>, ApiError> {
    let pool = &deployment.db().pool;
    let workspace = Workspace::find_by_id(pool, query.workspace_id)
        .await?
        .ok_or_else(|| ApiError::BadRequest("Workspace not found".to_string()))?;

    let container_ref = workspace.container_ref.unwrap_or_default();
    let mcp_path = std::path::PathBuf::from(&container_ref).join(".mcp.json");

    if let Some(parent) = mcp_path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| ApiError::Io(e))?;
    }

    let mcpc = project_mcp_config();
    let config = serde_json::json!({ "mcpServers": body.servers });
    write_agent_config(&mcp_path, &mcpc, &config)
        .await
        .map_err(|e| ApiError::BadRequest(format!("Failed to write .mcp.json: {}", e)))?;

    Ok(ResponseJson(ProjectMcpResponse {
        servers: body.servers,
        config_path: mcp_path.to_string_lossy().to_string(),
    }))
}

pub fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route(
            "/project-mcp-config",
            get(get_project_mcp).post(update_project_mcp),
        )
}

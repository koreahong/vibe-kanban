use axum::{
    Json, Router,
    extract::Query,
    response::Json as ResponseJson,
    routing::{get, post},
};
use jira::{
    client::JiraClient,
    config::{self, JiraConfig},
    mapper::{map_jira_issue_to_vk, map_vk_issue_to_jira_fields},
};
use serde::{Deserialize, Serialize};
use utils::response::ApiResponse;

use crate::{DeploymentImpl, error::ApiError};

// --- Request/Response types ---

#[derive(Debug, Deserialize)]
pub struct JiraSearchParams {
    pub q: Option<String>,
    #[serde(rename = "type")]
    pub issue_type: Option<String>,
    pub max: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct JiraSearchResult {
    pub key: String,
    pub summary: String,
    pub status: String,
    pub priority: String,
    pub assignee: Option<String>,
    pub issuetype: String,
    pub parent_key: Option<String>,
    pub updated: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct JiraSearchResponse {
    pub total: usize,
    pub issues: Vec<JiraSearchResult>,
}

#[derive(Debug, Deserialize)]
pub struct JiraImportRequest {
    pub jira_key: String,
    pub project_id: String,
}

#[derive(Debug, Serialize)]
pub struct JiraImportResponse {
    pub jira_key: String,
    pub title: String,
    pub status: String,
    pub priority: String,
}

#[derive(Debug, Deserialize)]
pub struct JiraImportEpicRequest {
    pub epic_key: String,
    pub organization_id: String,
    pub include_issues: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct JiraImportEpicResponse {
    pub project_name: String,
    pub issues_imported: usize,
    pub issues: Vec<ImportedIssue>,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ImportedIssue {
    pub jira_key: String,
    pub title: String,
}

#[derive(Debug, Deserialize)]
pub struct JiraPushRequest {
    pub issue_id: String,
    pub project_key: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct JiraPushResponse {
    pub jira_key: String,
    pub jira_url: String,
}

// --- Helpers ---

fn load_config_or_err() -> Result<JiraConfig, ApiError> {
    config::load_config().ok_or_else(|| {
        ApiError::BadRequest("Jira not configured. Set config via PUT /api/jira/config first.".into())
    })
}

fn make_client(cfg: &JiraConfig) -> JiraClient {
    JiraClient::new(&cfg.jira_base_url, &cfg.jira_email, &cfg.jira_api_token)
}

fn build_jql(query: &str, issue_type: Option<&str>, project_key: &str) -> String {
    let key_pattern = regex::Regex::new(r"^[A-Z]+-\d+$").unwrap();
    let mut conditions = vec![format!("project = {}", project_key)];

    if key_pattern.is_match(query) {
        conditions.push(format!("key = \"{}\"", query));
    } else if !query.is_empty() {
        conditions.push(format!("text ~ \"{}\"", query));
    }

    if let Some(it) = issue_type {
        conditions.push(format!("issuetype = \"{}\"", it));
    }

    format!("{} ORDER BY updated DESC", conditions.join(" AND "))
}

// --- Handlers ---

async fn get_config() -> Result<ResponseJson<ApiResponse<Option<JiraConfig>>>, ApiError> {
    Ok(ResponseJson(ApiResponse::success(config::load_config())))
}

async fn put_config(
    Json(payload): Json<JiraConfig>,
) -> Result<ResponseJson<ApiResponse<JiraConfig>>, ApiError> {
    config::save_config(&payload)
        .map_err(|e| ApiError::BadRequest(format!("Failed to save config: {e}")))?;
    Ok(ResponseJson(ApiResponse::success(payload)))
}

async fn search(
    Query(params): Query<JiraSearchParams>,
) -> Result<ResponseJson<ApiResponse<JiraSearchResponse>>, ApiError> {
    let cfg = load_config_or_err()?;
    let client = make_client(&cfg);

    let query = params.q.unwrap_or_default();
    let jql = build_jql(&query, params.issue_type.as_deref(), &cfg.jira_project_key);
    let max = params.max.unwrap_or(10);

    let response = client
        .search_issues(&jql, None, max)
        .await
        .map_err(|e| ApiError::BadRequest(format!("Jira search failed: {e}")))?;

    let issues: Vec<JiraSearchResult> = response
        .issues
        .iter()
        .map(|issue| {
            let f = &issue.fields;
            JiraSearchResult {
                key: issue.key.clone(),
                summary: f.summary.clone().unwrap_or_default(),
                status: f.status.as_ref().map(|s| s.name.clone()).unwrap_or_default(),
                priority: f.priority.as_ref().map(|p| p.name.clone()).unwrap_or_default(),
                assignee: f.assignee.as_ref().and_then(|a| a.display_name.clone()),
                issuetype: f.issuetype.as_ref().map(|t| t.name.clone()).unwrap_or_default(),
                parent_key: f.parent.as_ref().map(|p| p.key.clone()),
                updated: f.updated.clone(),
            }
        })
        .collect();

    let total = issues.len();
    Ok(ResponseJson(ApiResponse::success(JiraSearchResponse {
        total,
        issues,
    })))
}

async fn import_issue(
    Json(payload): Json<JiraImportRequest>,
) -> Result<ResponseJson<ApiResponse<JiraImportResponse>>, ApiError> {
    let cfg = load_config_or_err()?;
    let client = make_client(&cfg);

    let jira_issue = client
        .get_issue(&payload.jira_key, None)
        .await
        .map_err(|e| ApiError::BadRequest(format!("Failed to fetch {}: {e}", payload.jira_key)))?;

    let vk_fields = map_jira_issue_to_vk(&jira_issue, &cfg.user_mappings);

    Ok(ResponseJson(ApiResponse::success(JiraImportResponse {
        jira_key: payload.jira_key,
        title: vk_fields.title,
        status: vk_fields.status,
        priority: vk_fields.priority,
    })))
}

async fn import_epic(
    Json(payload): Json<JiraImportEpicRequest>,
) -> Result<ResponseJson<ApiResponse<JiraImportEpicResponse>>, ApiError> {
    let cfg = load_config_or_err()?;
    let client = make_client(&cfg);

    let epic = client
        .get_issue(&payload.epic_key, None)
        .await
        .map_err(|e| ApiError::BadRequest(format!("Failed to fetch Epic {}: {e}", payload.epic_key)))?;

    let epic_summary = epic
        .fields
        .summary
        .as_deref()
        .unwrap_or("Untitled Epic")
        .to_string();

    let include_issues = payload.include_issues.unwrap_or(true);
    let mut imported = Vec::new();
    let mut errors = Vec::new();

    if include_issues {
        let child_jql = format!("parent = {} ORDER BY created ASC", payload.epic_key);
        match client.get_all_issues(&child_jql, None).await {
            Ok(children) => {
                for child in &children {
                    let vk_fields = map_jira_issue_to_vk(child, &cfg.user_mappings);
                    imported.push(ImportedIssue {
                        jira_key: child.key.clone(),
                        title: vk_fields.title,
                    });
                }
            }
            Err(e) => errors.push(format!("Failed to fetch child issues: {e}")),
        }
    }

    let issues_imported = imported.len();
    Ok(ResponseJson(ApiResponse::success(JiraImportEpicResponse {
        project_name: epic_summary,
        issues_imported,
        issues: imported,
        errors,
    })))
}

async fn push_issue(
    Json(payload): Json<JiraPushRequest>,
) -> Result<ResponseJson<ApiResponse<JiraPushResponse>>, ApiError> {
    let cfg = load_config_or_err()?;
    let client = make_client(&cfg);

    let project_key = payload
        .project_key
        .unwrap_or_else(|| cfg.jira_project_key.clone());

    // Build minimal Jira fields — actual VK issue fetch happens on frontend side
    let fields = map_vk_issue_to_jira_fields(
        &payload.issue_id, // Title will be overridden by frontend
        None,
        None,
        &project_key,
    );

    let created = client
        .create_issue(&fields)
        .await
        .map_err(|e| ApiError::BadRequest(format!("Failed to create Jira issue: {e}")))?;

    let jira_url = format!("{}/browse/{}", cfg.jira_base_url, created.key);
    Ok(ResponseJson(ApiResponse::success(JiraPushResponse {
        jira_key: created.key,
        jira_url,
    })))
}

// --- Router ---

pub fn router() -> Router<DeploymentImpl> {
    let inner = Router::new()
        .route("/config", get(get_config).put(put_config))
        .route("/search", get(search))
        .route("/import", post(import_issue))
        .route("/import-epic", post(import_epic))
        .route("/push", post(push_issue));

    Router::new().nest("/jira", inner)
}

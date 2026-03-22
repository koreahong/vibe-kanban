use api_types::{IssuePriority, IssueRelationshipType};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use axum_extra::headers::{Authorization, HeaderMapExt, authorization::Bearer};
use jira::{
    adf::adf_to_markdown,
    client::JiraClient,
    config::{self, JiraConfig},
    mapper::{map_jira_issue_to_vk, map_jira_link_type_to_vk, map_vk_issue_to_jira_fields},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    AppState,
    db::{
        issue_comments::IssueCommentRepository,
        issue_relationships::IssueRelationshipRepository,
        issues::IssueRepository,
        project_statuses::ProjectStatusRepository,
    },
};
use super::error::ErrorResponse;

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
    pub parent_summary: Option<String>,
    pub updated: Option<String>,
    pub duedate: Option<String>,
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

fn load_config_or_err() -> Result<JiraConfig, ErrorResponse> {
    config::load_config().ok_or_else(|| {
        ErrorResponse::new(
            StatusCode::BAD_REQUEST,
            "Jira not configured. Set config via PUT /api/jira/config first.",
        )
    })
}

fn make_client(cfg: &JiraConfig) -> JiraClient {
    JiraClient::new(&cfg.jira_base_url, &cfg.jira_email, &cfg.jira_api_token)
}

static KEY_PATTERN: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"^[A-Z]+-\d+$").unwrap());

fn build_jql(query: &str, issue_type: Option<&str>, project_key: &str) -> String {
    let query_upper = query.to_uppercase();
    let mut conditions = vec![format!("project = {}", project_key)];

    if !KEY_PATTERN.is_match(&query_upper) && !query.is_empty() {
        conditions.push(format!("summary ~ \"{}\"", query));
    }

    if let Some(it) = issue_type {
        conditions.push(format!("issuetype = \"{}\"", it));
    }

    format!("{} ORDER BY updated DESC", conditions.join(" AND "))
}

// --- Handlers ---

async fn get_config() -> Json<Option<JiraConfig>> {
    Json(config::load_config())
}

async fn put_config(
    Json(payload): Json<JiraConfig>,
) -> Result<Json<JiraConfig>, ErrorResponse> {
    config::save_config(&payload).map_err(|e| {
        ErrorResponse::new(StatusCode::BAD_REQUEST, format!("Failed to save config: {e}"))
    })?;
    Ok(Json(payload))
}

fn jira_issue_to_search_result(issue: &jira::client::JiraIssue) -> JiraSearchResult {
    let f = &issue.fields;
    JiraSearchResult {
        key: issue.key.clone(),
        summary: f.summary.clone().unwrap_or_default(),
        status: f.status.as_ref().map(|s| s.name.clone()).unwrap_or_default(),
        priority: f.priority.as_ref().map(|p| p.name.clone()).unwrap_or_default(),
        assignee: f.assignee.as_ref().and_then(|a| a.display_name.clone()),
        issuetype: f.issuetype.as_ref().map(|t| t.name.clone()).unwrap_or_default(),
        parent_key: f.parent.as_ref().map(|p| p.key.clone()),
        parent_summary: f.parent.as_ref().and_then(|p| {
            p.fields.as_ref().and_then(|pf| pf.summary.clone())
        }),
        updated: f.updated.clone(),
        duedate: f.duedate.clone(),
    }
}

async fn search(
    Query(params): Query<JiraSearchParams>,
) -> Result<Json<JiraSearchResponse>, ErrorResponse> {
    let cfg = load_config_or_err()?;
    let client = make_client(&cfg);

    let query = params.q.unwrap_or_default();
    let query_upper = query.to_uppercase();

    // key 패턴이면 get_issue 직접 호출 (100-300ms vs 1-3s)
    if KEY_PATTERN.is_match(&query_upper) {
        match client.get_issue(&query_upper).await {
            Ok(issue) => {
                let result = jira_issue_to_search_result(&issue);
                return Ok(Json(JiraSearchResponse { total: 1, issues: vec![result] }));
            }
            Err(_) => {
                return Ok(Json(JiraSearchResponse { total: 0, issues: vec![] }));
            }
        }
    }

    let jql = build_jql(&query, params.issue_type.as_deref(), &cfg.jira_project_key);
    let max = params.max.unwrap_or(10);

    // Only request fields needed for display — skip description/subtasks/issuelinks
    let search_fields = &["summary", "status", "priority", "assignee", "issuetype", "parent", "updated", "duedate"];
    let response = client
        .search_issues(&jql, Some(search_fields), max)
        .await
        .map_err(|e| ErrorResponse::new(StatusCode::BAD_REQUEST, format!("Jira search failed: {e}")))?;

    let issues: Vec<JiraSearchResult> = response
        .issues
        .iter()
        .map(jira_issue_to_search_result)
        .collect();

    let total = issues.len();
    Ok(Json(JiraSearchResponse { total, issues }))
}

fn map_priority(priority_str: &str) -> Option<IssuePriority> {
    match priority_str {
        "urgent" => Some(IssuePriority::Urgent),
        "high" => Some(IssuePriority::High),
        "low" => Some(IssuePriority::Low),
        "lowest" => Some(IssuePriority::Lowest),
        _ => Some(IssuePriority::Medium),
    }
}

fn resolve_status_id(
    statuses: &[api_types::ProjectStatus],
    vk_status: &str,
) -> Option<Uuid> {
    let candidates: &[&str] = match vk_status {
        "todo" => &["To do", "To Do", "Backlog"],
        "inprogress" => &["In progress", "In Progress"],
        "done" => &["Done"],
        "cancelled" => &["Cancelled", "Canceled"],
        _ => &["To do", "To Do", "Backlog"],
    };
    statuses
        .iter()
        .find(|s| candidates.iter().any(|n| s.name.eq_ignore_ascii_case(n)))
        .or_else(|| statuses.iter().find(|s| !s.hidden))
        .or_else(|| statuses.first())
        .map(|s| s.id)
}

async fn import_issue(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<JiraImportRequest>,
) -> Result<Json<JiraImportResponse>, ErrorResponse> {
    let cfg = load_config_or_err()?;
    let client = make_client(&cfg);

    // Require auth via Bearer token
    let creator_user_id = {
        let bearer = headers
            .typed_get::<Authorization<Bearer>>()
            .ok_or_else(|| ErrorResponse::new(StatusCode::UNAUTHORIZED, "Authentication required for Jira import"))?;
        match crate::auth::request_context_from_access_token(&state, bearer.0.token()).await {
            Ok(ctx) => ctx.user.id,
            Err(_) => return Err(ErrorResponse::new(StatusCode::UNAUTHORIZED, "Invalid auth token")),
        }
    };

    // Parse project_id
    let project_id: Uuid = payload.project_id.parse().map_err(|_| {
        ErrorResponse::new(StatusCode::BAD_REQUEST, "Invalid project_id UUID")
    })?;

    // Fetch issue from Jira
    let jira_issue = client
        .get_issue(&payload.jira_key)
        .await
        .map_err(|e| ErrorResponse::new(StatusCode::BAD_REQUEST, format!("Failed to fetch {}: {e}", payload.jira_key)))?;

    let vk_fields = map_jira_issue_to_vk(&jira_issue, &cfg.user_mappings);

    // Load project statuses once (reused for sub-issues too)
    let statuses = ProjectStatusRepository::list_by_project(state.pool(), project_id)
        .await
        .map_err(|e| ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to load statuses: {e}")))?;

    let status_id = resolve_status_id(&statuses, &vk_fields.status)
        .ok_or_else(|| ErrorResponse::new(StatusCode::BAD_REQUEST, "Project has no statuses"))?;

    let description = if vk_fields.description.is_empty() {
        None
    } else {
        Some(vk_fields.description)
    };

    // Create main issue in DB
    let response = IssueRepository::create(
        state.pool(),
        None,
        project_id,
        status_id,
        vk_fields.title.clone(),
        description,
        map_priority(&vk_fields.priority),
        Some(vk_fields.simple_id.clone()),
        vk_fields.issue_number,
        None, None, None,
        0.0,
        None, None,
        serde_json::json!({}),
        creator_user_id,
    )
    .await
    .map_err(|e| ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to create issue: {e}")))?;

    let issue_id = response.data.id;

    // --- 3-A: Import sub-issues ---
    if let Some(subtasks) = &jira_issue.fields.subtasks {
        for subtask_ref in subtasks {
            let sub_key = &subtask_ref.key;
            // Skip if already imported into this project
            if IssueRepository::find_by_simple_id_in_project(state.pool(), sub_key, project_id)
                .await
                .ok()
                .flatten()
                .is_some()
            {
                continue;
            }
            match client.get_issue(sub_key).await {
                Ok(sub_issue) => {
                    let sub_fields = map_jira_issue_to_vk(&sub_issue, &cfg.user_mappings);
                    let sub_status_id = match resolve_status_id(&statuses, &sub_fields.status) {
                        Some(id) => id,
                        None => continue,
                    };
                    let sub_desc = if sub_fields.description.is_empty() {
                        None
                    } else {
                        Some(sub_fields.description)
                    };
                    let _ = IssueRepository::create(
                        state.pool(),
                        None,
                        project_id,
                        sub_status_id,
                        sub_fields.title,
                        sub_desc,
                        map_priority(&sub_fields.priority),
                        Some(sub_fields.simple_id),
                        sub_fields.issue_number,
                        None, None, None,
                        0.0,
                        Some(issue_id), None,
                        serde_json::json!({}),
                        creator_user_id,
                    )
                    .await;
                }
                Err(_) => {} // skip fetch failures
            }
        }
    }

    // --- 3-B: Import issue relationships ---
    if let Some(issuelinks) = &jira_issue.fields.issuelinks {
        for link in issuelinks {
            let link_type_name = link
                .link_type
                .as_ref()
                .map(|lt| lt.name.as_str())
                .unwrap_or("Relates");
            let rel_type_str = map_jira_link_type_to_vk(link_type_name);
            let rel_type = match rel_type_str {
                "blocking" => IssueRelationshipType::Blocking,
                "has_duplicate" => IssueRelationshipType::HasDuplicate,
                _ => IssueRelationshipType::Related,
            };

            // Try inward or outward issue
            let related_key = link
                .inward_issue
                .as_ref()
                .map(|i| i.key.as_str())
                .or_else(|| link.outward_issue.as_ref().map(|i| i.key.as_str()));

            if let Some(key) = related_key {
                if let Ok(Some(related)) =
                    IssueRepository::find_by_simple_id_in_project(state.pool(), key, project_id).await
                {
                    let _ = IssueRelationshipRepository::create(
                        state.pool(),
                        None,
                        issue_id,
                        related.id,
                        rel_type,
                    )
                    .await;
                }
                // Skip if related issue not yet in VK
            }
        }
    }

    // --- 3-C: Import comments ---
    if let Ok(comments) = client.get_issue_comments(&payload.jira_key).await {
        for comment in comments {
            let body_md = comment
                .body
                .as_ref()
                .map(|b| adf_to_markdown(b))
                .unwrap_or_default();
            if body_md.is_empty() {
                continue;
            }
            let _ = IssueCommentRepository::create(
                state.pool(),
                None,
                issue_id,
                creator_user_id,
                None,
                body_md,
            )
            .await;
        }
    }

    Ok(Json(JiraImportResponse {
        jira_key: payload.jira_key,
        title: response.data.title,
        status: vk_fields.status,
        priority: vk_fields.priority,
    }))
}

async fn import_epic(
    Json(payload): Json<JiraImportEpicRequest>,
) -> Result<Json<JiraImportEpicResponse>, ErrorResponse> {
    let cfg = load_config_or_err()?;
    let client = make_client(&cfg);

    let epic = client
        .get_issue(&payload.epic_key)
        .await
        .map_err(|e| ErrorResponse::new(StatusCode::BAD_REQUEST, format!("Failed to fetch Epic {}: {e}", payload.epic_key)))?;

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
    Ok(Json(JiraImportEpicResponse {
        project_name: epic_summary,
        issues_imported,
        issues: imported,
        errors,
    }))
}

async fn push_issue(
    Json(payload): Json<JiraPushRequest>,
) -> Result<Json<JiraPushResponse>, ErrorResponse> {
    let cfg = load_config_or_err()?;
    let client = make_client(&cfg);

    let project_key = payload
        .project_key
        .unwrap_or_else(|| cfg.jira_project_key.clone());

    let fields = map_vk_issue_to_jira_fields(
        &payload.issue_id,
        None,
        None,
        &project_key,
    );

    let created = client
        .create_issue(&fields)
        .await
        .map_err(|e| ErrorResponse::new(StatusCode::BAD_REQUEST, format!("Failed to create Jira issue: {e}")))?;

    let jira_url = format!("{}/browse/{}", cfg.jira_base_url, created.key);
    Ok(Json(JiraPushResponse {
        jira_key: created.key,
        jira_url,
    }))
}

// --- Router ---

pub fn router() -> Router<AppState> {
    let inner = Router::new()
        .route("/config", get(get_config).put(put_config))
        .route("/search", get(search))
        .route("/import", post(import_issue))
        .route("/import-epic", post(import_epic))
        .route("/push", post(push_issue));

    Router::new().nest("/jira", inner)
}

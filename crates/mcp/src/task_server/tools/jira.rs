use rmcp::{
    ErrorData, handler::server::tool::Parameters, model::CallToolResult, schemars, tool,
    tool_router,
};
use serde::{Deserialize, Serialize};

use super::McpServer;
use crate::jira::client::JiraClient;
use crate::jira::engine::{
    EpicMapping, IssueMapping, SyncConfig, SyncEngine, SyncReport, SyncState, VkApi,
    VkIssueSummary, VkProjectSummary,
};
use crate::jira::mapper::{UserMapping, map_jira_issue_to_vk, map_vk_issue_to_jira_fields};

// --- MCP Request/Response types ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct McpJiraConfigureRequest {
    #[schemars(description = "Jira instance base URL (e.g., 'https://yourcompany.atlassian.net')")]
    jira_base_url: String,
    #[schemars(description = "Jira user email for API authentication")]
    jira_email: String,
    #[schemars(description = "Jira API token for authentication")]
    jira_api_token: String,
    #[schemars(
        description = "Jira project key to sync (e.g., 'MPD'). Defaults to 'MPD' if not specified."
    )]
    jira_project_key: Option<String>,
    #[schemars(
        description = "VK organization ID. Jira Epics become Projects under this organization. Required."
    )]
    organization_id: String,
    #[schemars(
        description = "User mappings between VK user IDs and Jira account IDs. JSON array of {vk_user_id, jira_account_id} objects."
    )]
    user_mappings: Option<Vec<UserMappingInput>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct UserMappingInput {
    #[schemars(description = "VK user UUID")]
    vk_user_id: String,
    #[schemars(description = "Jira account ID")]
    jira_account_id: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct McpJiraConfigureResponse {
    #[schemars(description = "Whether the configuration was saved successfully")]
    success: bool,
    #[schemars(description = "Configuration summary")]
    message: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct McpJiraSyncRequest {
    #[schemars(
        description = "Run in dry-run mode (preview changes without applying). Defaults to true for safety."
    )]
    dry_run: Option<bool>,
    #[schemars(description = "Sync direction: 'both' (default), 'jira_to_vk', or 'vk_to_jira'")]
    direction: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct McpJiraSyncResponse {
    report: SyncReportSummary,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct SyncReportSummary {
    #[schemars(description = "Whether this was a dry run")]
    dry_run: bool,
    #[schemars(description = "Total issue mappings after sync")]
    mappings_total: usize,
    #[schemars(description = "Total Epic→Project mappings")]
    epic_mappings_total: usize,
    #[schemars(description = "Jira → VK direction results")]
    jira_to_vk: DirectionSummary,
    #[schemars(description = "VK → Jira direction results")]
    vk_to_jira: DirectionSummary,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct DirectionSummary {
    created: usize,
    updated: usize,
    skipped: usize,
    projects_created: usize,
    error_count: usize,
    errors: Vec<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct McpJiraStatusResponse {
    #[schemars(description = "Whether Jira sync is configured")]
    configured: bool,
    #[schemars(description = "Jira base URL if configured")]
    jira_base_url: Option<String>,
    #[schemars(description = "Jira project key if configured")]
    jira_project_key: Option<String>,
    #[schemars(description = "VK organization ID if configured")]
    organization_id: Option<String>,
    #[schemars(description = "Last sync timestamp")]
    last_sync: Option<String>,
    #[schemars(description = "Number of issue mappings")]
    mappings_count: usize,
    #[schemars(description = "Number of Epic→Project mappings")]
    epic_mappings_count: usize,
}

// --- jira_search types ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct McpJiraSearchRequest {
    #[schemars(description = "Search query. If it matches a Jira key pattern (e.g., 'MPD-172'), searches by key. Otherwise searches by text.")]
    query: String,
    #[schemars(description = "Maximum results to return. Defaults to 10.")]
    max_results: Option<u32>,
    #[schemars(description = "Filter by issue type (e.g., 'Epic', 'Task', 'Story', 'Bug'). If omitted, all types returned.")]
    issue_type: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct JiraSearchResultItem {
    key: String,
    summary: String,
    status: String,
    priority: String,
    assignee: Option<String>,
    issuetype: String,
    parent_key: Option<String>,
    updated: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct McpJiraSearchResponse {
    #[schemars(description = "Number of results found")]
    total: usize,
    issues: Vec<JiraSearchResultItem>,
}

// --- jira_import types ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct McpJiraImportRequest {
    #[schemars(description = "Jira issue key to import (e.g., 'MPD-172')")]
    jira_key: String,
    #[schemars(description = "VK project UUID to import the issue into")]
    project_id: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct McpJiraImportResponse {
    #[schemars(description = "Created VK issue UUID")]
    vk_issue_id: String,
    #[schemars(description = "VK issue simple_id (= Jira key)")]
    simple_id: String,
    #[schemars(description = "Issue title")]
    title: String,
}

// --- jira_import_epic types ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct McpJiraImportEpicRequest {
    #[schemars(description = "Jira Epic key to import (e.g., 'MPD-105')")]
    epic_key: String,
    #[schemars(description = "VK organization UUID to create the project under")]
    organization_id: String,
    #[schemars(description = "Whether to import child issues under the Epic. Defaults to true.")]
    include_issues: Option<bool>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct ImportedIssueSummary {
    jira_key: String,
    title: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct McpJiraImportEpicResponse {
    #[schemars(description = "Created VK project UUID")]
    vk_project_id: String,
    #[schemars(description = "Project name (Epic summary)")]
    project_name: String,
    #[schemars(description = "Number of child issues imported")]
    issues_imported: usize,
    #[schemars(description = "Imported issue details")]
    issues: Vec<ImportedIssueSummary>,
    #[schemars(description = "Errors encountered during child issue import")]
    errors: Vec<String>,
}

// --- jira_push types ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct McpJiraPushRequest {
    #[schemars(description = "VK issue UUID to push to Jira")]
    issue_id: String,
    #[schemars(description = "Jira project key (e.g., 'MPD'). Uses configured default if omitted.")]
    project_key: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct McpJiraPushResponse {
    #[schemars(description = "Created Jira issue key")]
    jira_key: String,
    #[schemars(description = "URL to view the issue in Jira")]
    jira_url: String,
}

// --- In-memory state (per MCP server instance) ---

use std::sync::Mutex;

static JIRA_CONFIG: Mutex<Option<SyncConfig>> = Mutex::new(None);
static JIRA_SYNC_STATE: Mutex<Option<SyncState>> = Mutex::new(None);

// --- VK API implementation using McpServer ---

#[allow(dead_code)]
struct McpVkApi<'a> {
    server: &'a McpServer,
    organization_id: String,
}

#[async_trait::async_trait]
impl VkApi for McpVkApi<'_> {
    async fn list_issues(&self, project_id: &str) -> Result<Vec<VkIssueSummary>, String> {
        let url = self.server.url(&format!(
            "/api/remote/issues?project_id={}",
            project_id
        ));
        let response: api_types::ListIssuesResponse = self
            .server
            .send_json(self.server.client().get(&url))
            .await
            .map_err(|_| "Failed to list VK issues".to_string())?;

        // Fetch status names
        let statuses_url = self.server.url(&format!(
            "/api/remote/project-statuses?project_id={}",
            project_id
        ));
        let status_map: std::collections::HashMap<uuid::Uuid, String> = self
            .server
            .send_json::<api_types::ListProjectStatusesResponse>(
                self.server.client().get(&statuses_url),
            )
            .await
            .map(|resp| {
                resp.project_statuses
                    .into_iter()
                    .map(|s| (s.id, s.name))
                    .collect()
            })
            .unwrap_or_default();

        Ok(response
            .issues
            .into_iter()
            .map(|i| {
                let status_name = status_map.get(&i.status_id).cloned();
                VkIssueSummary {
                    id: i.id.to_string(),
                    title: i.title,
                    simple_id: i.simple_id,
                    description: i.description,
                    status: status_name,
                    priority: i
                        .priority
                        .map(|p| format!("{:?}", p).to_lowercase()),
                    updated_at: Some(i.updated_at.to_rfc3339()),
                    assignee_id: None, // Would need separate fetch
                }
            })
            .collect())
    }

    async fn create_issue(
        &self,
        project_id: &str,
        title: &str,
        description: Option<&str>,
        status: Option<&str>,
        priority: Option<&str>,
    ) -> Result<String, String> {
        self.create_issue_with_jira_key(project_id, title, description, status, priority, "", 0)
            .await
    }

    async fn create_issue_with_jira_key(
        &self,
        project_id: &str,
        title: &str,
        description: Option<&str>,
        status: Option<&str>,
        priority: Option<&str>,
        simple_id: &str,
        issue_number: i32,
    ) -> Result<String, String> {
        let project_id: uuid::Uuid = project_id
            .parse()
            .map_err(|_| "Invalid project ID".to_string())?;

        // Resolve status
        let status_id = if let Some(status_name) = status {
            self.server
                .resolve_status_id(project_id, status_name)
                .await
                .map_err(|_| format!("Failed to resolve status: {}", status_name))?
        } else {
            self.server
                .default_status_id(project_id)
                .await
                .map_err(|_| "Failed to get default status".to_string())?
        };

        let priority = priority
            .map(|p| McpServer::parse_issue_priority(p))
            .transpose()
            .map_err(|_| "Invalid priority".to_string())?;

        let simple_id_opt = if simple_id.is_empty() {
            None
        } else {
            Some(simple_id.to_string())
        };
        let issue_number_opt = if issue_number > 0 {
            Some(issue_number)
        } else {
            None
        };

        let payload = api_types::CreateIssueRequest {
            id: None,
            project_id,
            status_id,
            title: title.to_string(),
            description: description.map(|d| d.to_string()),
            priority,
            simple_id: simple_id_opt,
            issue_number: issue_number_opt,
            start_date: None,
            target_date: None,
            completed_at: None,
            sort_order: 0.0,
            parent_issue_id: None,
            parent_issue_sort_order: None,
            extension_metadata: serde_json::json!({}),
        };

        let url = self.server.url("/api/remote/issues");
        let response: api_types::MutationResponse<api_types::Issue> = self
            .server
            .send_json(self.server.client().post(&url).json(&payload))
            .await
            .map_err(|_| "Failed to create VK issue".to_string())?;

        Ok(response.data.id.to_string())
    }

    async fn update_issue(
        &self,
        issue_id: &str,
        title: Option<&str>,
        description: Option<&str>,
        status: Option<&str>,
        priority: Option<&str>,
    ) -> Result<(), String> {
        let issue_uuid: uuid::Uuid = issue_id
            .parse()
            .map_err(|_| "Invalid issue ID".to_string())?;

        // First get issue to know project_id
        let get_url = self.server.url(&format!("/api/remote/issues/{}", issue_id));
        let existing: api_types::Issue = self
            .server
            .send_json(self.server.client().get(&get_url))
            .await
            .map_err(|_| "Failed to fetch existing issue".to_string())?;

        let status_id = if let Some(status_name) = status {
            Some(
                self.server
                    .resolve_status_id(existing.project_id, status_name)
                    .await
                    .map_err(|_| format!("Failed to resolve status: {}", status_name))?,
            )
        } else {
            None
        };

        let priority = if let Some(p) = priority {
            Some(Some(
                McpServer::parse_issue_priority(p)
                    .map_err(|_| "Invalid priority".to_string())?,
            ))
        } else {
            None
        };

        let payload = api_types::UpdateIssueRequest {
            status_id,
            title: title.map(|t| t.to_string()),
            description: description.map(|d| Some(d.to_string())),
            priority,
            start_date: None,
            target_date: None,
            completed_at: None,
            sort_order: None,
            parent_issue_id: None,
            parent_issue_sort_order: None,
            extension_metadata: None,
        };

        let url = self.server.url(&format!("/api/remote/issues/{}", issue_uuid));
        let _: api_types::MutationResponse<api_types::Issue> = self
            .server
            .send_json(self.server.client().patch(&url).json(&payload))
            .await
            .map_err(|_| "Failed to update VK issue".to_string())?;

        Ok(())
    }

    async fn assign_issue(&self, issue_id: &str, user_id: &str) -> Result<(), String> {
        let url = self.server.url("/api/remote/issue-assignees");
        let payload = serde_json::json!({
            "issue_id": issue_id,
            "user_id": user_id,
        });
        self.server
            .send_json::<serde_json::Value>(self.server.client().post(&url).json(&payload))
            .await
            .map_err(|_| "Failed to assign issue".to_string())?;
        Ok(())
    }

    async fn add_issue_tag(&self, issue_id: &str, tag_name: &str) -> Result<(), String> {
        let _ = (issue_id, tag_name);
        Ok(())
    }

    async fn create_issue_relationship(
        &self,
        issue_id: &str,
        related_issue_id: &str,
        relationship_type: &str,
    ) -> Result<(), String> {
        let url = self.server.url("/api/remote/issue-relationships");
        let payload = serde_json::json!({
            "issue_id": issue_id,
            "related_issue_id": related_issue_id,
            "relationship_type": relationship_type,
        });
        self.server
            .send_json::<serde_json::Value>(self.server.client().post(&url).json(&payload))
            .await
            .map_err(|_| "Failed to create relationship".to_string())?;
        Ok(())
    }

    async fn update_issue_parent(
        &self,
        issue_id: &str,
        parent_issue_id: &str,
    ) -> Result<(), String> {
        self.update_issue(issue_id, None, None, None, None).await?;
        let _ = parent_issue_id;
        Ok(())
    }

    async fn create_project(
        &self,
        organization_id: &str,
        name: &str,
    ) -> Result<String, String> {
        let org_id: uuid::Uuid = organization_id
            .parse()
            .map_err(|_| "Invalid organization ID".to_string())?;

        let payload = api_types::CreateProjectRequest {
            id: None,
            organization_id: org_id,
            name: name.to_string(),
            color: "217 91% 60%".to_string(), // Default blue
        };

        let url = self.server.url("/api/remote/projects");
        let response: api_types::MutationResponse<api_types::Project> = self
            .server
            .send_json(self.server.client().post(&url).json(&payload))
            .await
            .map_err(|_| "Failed to create VK project".to_string())?;

        Ok(response.data.id.to_string())
    }

    async fn list_projects(
        &self,
        organization_id: &str,
    ) -> Result<Vec<VkProjectSummary>, String> {
        let url = self.server.url(&format!(
            "/api/remote/projects?organization_id={}",
            organization_id
        ));
        let response: api_types::ListProjectsResponse = self
            .server
            .send_json(self.server.client().get(&url))
            .await
            .map_err(|_| "Failed to list VK projects".to_string())?;

        Ok(response
            .projects
            .into_iter()
            .map(|p| VkProjectSummary {
                id: p.id.to_string(),
                name: p.name,
            })
            .collect())
    }
}

fn report_to_summary(report: &SyncReport) -> SyncReportSummary {
    SyncReportSummary {
        dry_run: report.dry_run,
        mappings_total: report.mappings_total,
        epic_mappings_total: report.epic_mappings_total,
        jira_to_vk: DirectionSummary {
            created: report.jira_to_vk.created,
            updated: report.jira_to_vk.updated,
            skipped: report.jira_to_vk.skipped,
            projects_created: report.jira_to_vk.projects_created,
            error_count: report.jira_to_vk.errors.len(),
            errors: report
                .jira_to_vk
                .errors
                .iter()
                .map(|e| format!("{}: {}", e.key, e.error))
                .collect(),
        },
        vk_to_jira: DirectionSummary {
            created: report.vk_to_jira.created,
            updated: report.vk_to_jira.updated,
            skipped: report.vk_to_jira.skipped,
            projects_created: report.vk_to_jira.projects_created,
            error_count: report.vk_to_jira.errors.len(),
            errors: report
                .vk_to_jira
                .errors
                .iter()
                .map(|e| format!("{}: {}", e.key, e.error))
                .collect(),
        },
    }
}

// --- Helpers ---

fn get_jira_client() -> Result<(JiraClient, SyncConfig), String> {
    let mut guard = JIRA_CONFIG.lock().unwrap();

    // Auto-load from persistent config if memory is empty
    if guard.is_none() {
        if let Some(persisted) = crate::jira::config::load_config() {
            *guard = Some(SyncConfig {
                jira_base_url: persisted.jira_base_url,
                jira_email: persisted.jira_email,
                jira_api_token: persisted.jira_api_token,
                jira_project_key: persisted.jira_project_key,
                organization_id: persisted.organization_id,
                user_mappings: persisted.user_mappings,
            });
        }
    }

    let config = guard
        .clone()
        .ok_or_else(|| "Jira not configured. Call jira_configure first.".to_string())?;
    let client = JiraClient::new(
        &config.jira_base_url,
        &config.jira_email,
        &config.jira_api_token,
    );
    Ok((client, config))
}

fn build_search_jql(query: &str, issue_type: Option<&str>, project_key: &str) -> String {
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

// --- MCP Tool implementations ---

#[tool_router(router = jira_tools_router, vis = "pub")]
impl McpServer {
    #[tool(
        description = "Configure Jira sync settings. Must be called before running jira_sync. Jira Epics become VK Projects under the specified organization."
    )]
    async fn jira_configure(
        &self,
        Parameters(req): Parameters<McpJiraConfigureRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let user_mappings: Vec<UserMapping> = req
            .user_mappings
            .unwrap_or_default()
            .into_iter()
            .map(|u| UserMapping {
                vk_user_id: u.vk_user_id,
                jira_account_id: u.jira_account_id,
            })
            .collect();

        let config = SyncConfig {
            jira_base_url: req.jira_base_url.clone(),
            jira_email: req.jira_email,
            jira_api_token: req.jira_api_token,
            jira_project_key: req.jira_project_key.unwrap_or_else(|| "MPD".to_string()),
            organization_id: req.organization_id.clone(),
            user_mappings,
        };

        // Persist to disk
        let persist_config = crate::jira::config::JiraConfig {
            jira_base_url: config.jira_base_url.clone(),
            jira_email: config.jira_email.clone(),
            jira_api_token: config.jira_api_token.clone(),
            jira_project_key: config.jira_project_key.clone(),
            organization_id: config.organization_id.clone(),
            user_mappings: config.user_mappings.clone(),
        };
        if let Err(e) = crate::jira::config::save_config(&persist_config) {
            tracing::warn!("Failed to persist Jira config: {e}");
        }

        *JIRA_CONFIG.lock().unwrap() = Some(config);

        McpServer::success(&McpJiraConfigureResponse {
            success: true,
            message: format!(
                "Jira sync configured: {} → VK organization {}. Config saved to ~/.vibe-kanban/jira-config.json",
                req.jira_base_url, req.organization_id
            ),
        })
    }

    #[tool(
        description = "Run Jira ↔ VK bidirectional sync. Call jira_configure first. Jira Epics become VK Projects; Tasks/Stories/Bugs become VK Issues (Epic-less issues are skipped). Issue simple_id = Jira key (e.g., MPD-172). Defaults to dry-run mode."
    )]
    async fn jira_sync(
        &self,
        Parameters(req): Parameters<McpJiraSyncRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let config = JIRA_CONFIG.lock().unwrap().clone();
        let config = match config {
            Some(c) => c,
            None => {
                return Ok(Self::err(
                    "Jira sync not configured. Call jira_configure first.",
                    None::<&str>,
                )
                .unwrap())
            }
        };

        let dry_run = req.dry_run.unwrap_or(true);
        let state = JIRA_SYNC_STATE
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_default();

        let mut engine = SyncEngine::new(config.clone(), state, dry_run);

        let vk_api = McpVkApi {
            server: self,
            organization_id: config.organization_id.clone(),
        };

        let direction = req.direction.as_deref().unwrap_or("both");
        let report = match direction {
            "jira_to_vk" => {
                let jira_to_vk = engine.sync_jira_to_vk(&vk_api).await;
                SyncReport {
                    jira_to_vk,
                    vk_to_jira: Default::default(),
                    dry_run,
                    mappings_total: engine.state().mappings.len(),
                    epic_mappings_total: engine.state().epic_mappings.len(),
                }
            }
            "vk_to_jira" => {
                let vk_to_jira = engine.sync_vk_to_jira(&vk_api).await;
                SyncReport {
                    jira_to_vk: Default::default(),
                    vk_to_jira,
                    dry_run,
                    mappings_total: engine.state().mappings.len(),
                    epic_mappings_total: engine.state().epic_mappings.len(),
                }
            }
            _ => engine.run_full_sync(&vk_api).await,
        };

        let summary = report_to_summary(&report);

        // Save state if not dry run
        if !dry_run {
            *JIRA_SYNC_STATE.lock().unwrap() = Some(engine.into_state());
        }

        McpServer::success(&McpJiraSyncResponse { report: summary })
    }

    #[tool(description = "Check Jira sync configuration status and last sync info.")]
    async fn jira_status(&self) -> Result<CallToolResult, ErrorData> {
        let config = JIRA_CONFIG.lock().unwrap().clone();
        let state = JIRA_SYNC_STATE.lock().unwrap().clone();

        McpServer::success(&McpJiraStatusResponse {
            configured: config.is_some(),
            jira_base_url: config.as_ref().map(|c| c.jira_base_url.clone()),
            jira_project_key: config.as_ref().map(|c| c.jira_project_key.clone()),
            organization_id: config.map(|c| c.organization_id),
            last_sync: state.as_ref().and_then(|s| s.last_sync.clone()),
            mappings_count: state.as_ref().map(|s| s.mappings.len()).unwrap_or(0),
            epic_mappings_count: state.map(|s| s.epic_mappings.len()).unwrap_or(0),
        })
    }

    #[tool(
        description = "Search Jira issues. Supports key lookup (e.g., 'MPD-172'), text search, and issue type filtering (e.g., issue_type='Epic'). Call jira_configure first."
    )]
    async fn jira_search(
        &self,
        Parameters(req): Parameters<McpJiraSearchRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let (client, config) = match get_jira_client() {
            Ok(v) => v,
            Err(e) => return Ok(Self::err(e, None::<String>).unwrap()),
        };

        let jql = build_search_jql(
            &req.query,
            req.issue_type.as_deref(),
            &config.jira_project_key,
        );
        let max_results = req.max_results.unwrap_or(10);

        match client.search_issues(&jql, None, max_results).await {
            Ok(response) => {
                let items: Vec<JiraSearchResultItem> = response
                    .issues
                    .iter()
                    .map(|issue| {
                        let f = &issue.fields;
                        JiraSearchResultItem {
                            key: issue.key.clone(),
                            summary: f.summary.clone().unwrap_or_default(),
                            status: f
                                .status
                                .as_ref()
                                .map(|s| s.name.clone())
                                .unwrap_or_default(),
                            priority: f
                                .priority
                                .as_ref()
                                .map(|p| p.name.clone())
                                .unwrap_or_default(),
                            assignee: f
                                .assignee
                                .as_ref()
                                .and_then(|a| a.display_name.clone()),
                            issuetype: f
                                .issuetype
                                .as_ref()
                                .map(|t| t.name.clone())
                                .unwrap_or_default(),
                            parent_key: f.parent.as_ref().map(|p| p.key.clone()),
                            updated: f.updated.clone(),
                        }
                    })
                    .collect();

                let total = items.len();
                McpServer::success(&McpJiraSearchResponse {
                    total,
                    issues: items,
                })
            }
            Err(e) => Ok(Self::err(format!("Jira search failed: {e}"), None::<String>).unwrap()),
        }
    }

    #[tool(
        description = "Import a single Jira issue into a VK project. The issue's simple_id is set to the Jira key (e.g., 'MPD-172'). Call jira_configure first."
    )]
    async fn jira_import(
        &self,
        Parameters(req): Parameters<McpJiraImportRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let (client, config) = match get_jira_client() {
            Ok(v) => v,
            Err(e) => return Ok(Self::err(e, None::<String>).unwrap()),
        };

        // 1. Fetch the Jira issue
        let jira_issue = match client.get_issue(&req.jira_key).await {
            Ok(issue) => issue,
            Err(e) => {
                return Ok(Self::err(
                    format!("Failed to fetch Jira issue {}: {e}", req.jira_key),
                    None::<String>,
                )
                .unwrap())
            }
        };

        // 2. Map to VK fields
        let vk_fields = map_jira_issue_to_vk(&jira_issue, &config.user_mappings);

        // 3. Create VK issue via VK API
        let vk_api = McpVkApi {
            server: self,
            organization_id: config.organization_id.clone(),
        };

        let issue_number = vk_fields.issue_number.unwrap_or(0);
        let vk_issue_id = match vk_api
            .create_issue_with_jira_key(
                &req.project_id,
                &vk_fields.title,
                Some(&vk_fields.description),
                Some(&vk_fields.status),
                Some(&vk_fields.priority),
                &vk_fields.simple_id,
                issue_number,
            )
            .await
        {
            Ok(id) => id,
            Err(e) => {
                return Ok(
                    Self::err(format!("Failed to create VK issue: {e}"), None::<String>).unwrap(),
                )
            }
        };

        // 4. Assign user if mapped
        if let Some(ref assignee_id) = vk_fields.assignee_vk_id {
            let _ = vk_api.assign_issue(&vk_issue_id, assignee_id).await;
        }

        // 5. Register mapping in sync state
        {
            let mut state_guard = JIRA_SYNC_STATE.lock().unwrap();
            let state = state_guard.get_or_insert_with(SyncState::default);
            state.mappings.insert(
                req.jira_key.clone(),
                IssueMapping {
                    vk_issue_id: vk_issue_id.clone(),
                    last_jira_update: jira_issue.fields.updated.clone(),
                    last_vk_update: Some(chrono::Utc::now().to_rfc3339()),
                },
            );
        }

        McpServer::success(&McpJiraImportResponse {
            vk_issue_id,
            simple_id: vk_fields.simple_id,
            title: vk_fields.title,
        })
    }

    #[tool(
        description = "Import a Jira Epic as a VK project, optionally including all child issues. Creates a new project and imports child Tasks/Stories/Bugs. Call jira_configure first."
    )]
    async fn jira_import_epic(
        &self,
        Parameters(req): Parameters<McpJiraImportEpicRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let (client, config) = match get_jira_client() {
            Ok(v) => v,
            Err(e) => return Ok(Self::err(e, None::<String>).unwrap()),
        };

        // 1. Fetch the Epic
        let epic = match client.get_issue(&req.epic_key).await {
            Ok(issue) => issue,
            Err(e) => {
                return Ok(Self::err(
                    format!("Failed to fetch Epic {}: {e}", req.epic_key),
                    None::<String>,
                )
                .unwrap())
            }
        };

        let epic_summary = epic
            .fields
            .summary
            .as_deref()
            .unwrap_or("Untitled Epic")
            .to_string();

        // 2. Create VK project
        let vk_api = McpVkApi {
            server: self,
            organization_id: req.organization_id.clone(),
        };

        let vk_project_id = match vk_api
            .create_project(&req.organization_id, &epic_summary)
            .await
        {
            Ok(id) => id,
            Err(e) => {
                return Ok(
                    Self::err(format!("Failed to create VK project: {e}"), None::<String>).unwrap(),
                )
            }
        };

        // 3. Register epic mapping
        {
            let mut state_guard = JIRA_SYNC_STATE.lock().unwrap();
            let state = state_guard.get_or_insert_with(SyncState::default);
            state.epic_mappings.insert(
                req.epic_key.clone(),
                EpicMapping {
                    vk_project_id: vk_project_id.clone(),
                    epic_summary: epic_summary.clone(),
                    last_jira_update: epic.fields.updated.clone(),
                },
            );
        }

        // 4. Import child issues if requested
        let include_issues = req.include_issues.unwrap_or(true);
        let mut imported_issues = Vec::new();
        let mut errors = Vec::new();

        if include_issues {
            // Search for child issues: parent = epic_key
            let child_jql = format!(
                "parent = {} ORDER BY created ASC",
                req.epic_key
            );

            match client.get_all_issues(&child_jql, None).await {
                Ok(children) => {
                    for child in &children {
                        let vk_fields = map_jira_issue_to_vk(child, &config.user_mappings);
                        let issue_number = vk_fields.issue_number.unwrap_or(0);

                        match vk_api
                            .create_issue_with_jira_key(
                                &vk_project_id,
                                &vk_fields.title,
                                Some(&vk_fields.description),
                                Some(&vk_fields.status),
                                Some(&vk_fields.priority),
                                &vk_fields.simple_id,
                                issue_number,
                            )
                            .await
                        {
                            Ok(vk_id) => {
                                // Assign user if mapped
                                if let Some(ref assignee_id) = vk_fields.assignee_vk_id {
                                    let _ =
                                        vk_api.assign_issue(&vk_id, assignee_id).await;
                                }

                                // Register mapping
                                {
                                    let mut state_guard = JIRA_SYNC_STATE.lock().unwrap();
                                    let state =
                                        state_guard.get_or_insert_with(SyncState::default);
                                    state.mappings.insert(
                                        child.key.clone(),
                                        IssueMapping {
                                            vk_issue_id: vk_id,
                                            last_jira_update: child.fields.updated.clone(),
                                            last_vk_update: Some(
                                                chrono::Utc::now().to_rfc3339(),
                                            ),
                                        },
                                    );
                                }

                                imported_issues.push(ImportedIssueSummary {
                                    jira_key: child.key.clone(),
                                    title: vk_fields.title,
                                });
                            }
                            Err(e) => {
                                errors.push(format!("{}: {e}", child.key));
                            }
                        }
                    }
                }
                Err(e) => {
                    errors.push(format!("Failed to fetch child issues: {e}"));
                }
            }
        }

        let issues_imported = imported_issues.len();
        McpServer::success(&McpJiraImportEpicResponse {
            vk_project_id,
            project_name: epic_summary,
            issues_imported,
            issues: imported_issues,
            errors,
        })
    }

    #[tool(
        description = "Push a VK issue to Jira, creating a new Jira issue. The created Jira key is saved in the VK issue's extension_metadata. Call jira_configure first."
    )]
    async fn jira_push(
        &self,
        Parameters(req): Parameters<McpJiraPushRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let (client, config) = match get_jira_client() {
            Ok(v) => v,
            Err(e) => return Ok(Self::err(e, None::<String>).unwrap()),
        };

        let issue_uuid: uuid::Uuid = match req.issue_id.parse() {
            Ok(id) => id,
            Err(_) => return Ok(Self::err("Invalid issue_id UUID", None::<&str>).unwrap()),
        };

        // 1. Fetch VK issue
        let get_url = self.url(&format!("/api/remote/issues/{}", issue_uuid));
        let vk_issue: api_types::Issue = match self.send_json(self.client().get(&get_url)).await {
            Ok(i) => i,
            Err(e) => return Ok(e),
        };

        // 2. Map to Jira fields
        let project_key = req
            .project_key
            .unwrap_or_else(|| config.jira_project_key.clone());

        let priority_str = vk_issue
            .priority
            .as_ref()
            .map(|p| format!("{:?}", p).to_lowercase());

        let jira_fields = map_vk_issue_to_jira_fields(
            &vk_issue.title,
            vk_issue.description.as_deref(),
            priority_str.as_deref(),
            &project_key,
        );

        // 3. Create in Jira
        let created = match client.create_issue(&jira_fields).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(
                    Self::err(format!("Failed to create Jira issue: {e}"), None::<String>).unwrap(),
                )
            }
        };

        let jira_key = created.key.clone();
        let jira_url = format!("{}/browse/{}", config.jira_base_url, jira_key);

        // 4. Update VK issue extension_metadata with jira_key
        let mut ext_meta = vk_issue.extension_metadata.clone();
        ext_meta["jira_key"] = serde_json::json!(jira_key);

        let patch_payload = api_types::UpdateIssueRequest {
            status_id: None,
            title: None,
            description: None,
            priority: None,
            start_date: None,
            target_date: None,
            completed_at: None,
            sort_order: None,
            parent_issue_id: None,
            parent_issue_sort_order: None,
            extension_metadata: Some(ext_meta),
        };

        let patch_url = self.url(&format!("/api/remote/issues/{}", issue_uuid));
        let _ = self
            .send_json::<api_types::MutationResponse<api_types::Issue>>(
                self.client().patch(&patch_url).json(&patch_payload),
            )
            .await;

        // 5. Register mapping in sync state
        {
            let mut state_guard = JIRA_SYNC_STATE.lock().unwrap();
            let state = state_guard.get_or_insert_with(SyncState::default);
            state.mappings.insert(
                jira_key.clone(),
                IssueMapping {
                    vk_issue_id: issue_uuid.to_string(),
                    last_jira_update: None,
                    last_vk_update: Some(chrono::Utc::now().to_rfc3339()),
                },
            );
        }

        McpServer::success(&McpJiraPushResponse { jira_key, jira_url })
    }
}

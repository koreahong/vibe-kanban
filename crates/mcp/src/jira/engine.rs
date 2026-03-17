//! Bidirectional sync engine between Jira and VK.
//!
//! Key mapping: Jira Epic → VK Project, Jira Task/Story/Bug → VK Issue.
//! Issues without an Epic parent are skipped.
//! VK issue simple_id is set to the Jira key (e.g., "MPD-172").

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use jira::client::JiraClient;
use jira::mapper::{
    UserMapping, extract_jira_key, map_jira_issue_to_vk, map_vk_priority_to_jira,
    map_vk_status_to_jira, map_vk_user_to_jira, vk_description_to_jira,
};

// --- Sync State ---

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncState {
    /// Jira key → mapping info
    pub mappings: HashMap<String, IssueMapping>,
    /// Jira Epic key → VK project mapping
    pub epic_mappings: HashMap<String, EpicMapping>,
    pub last_sync: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueMapping {
    pub vk_issue_id: String,
    pub last_jira_update: Option<String>,
    pub last_vk_update: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpicMapping {
    pub vk_project_id: String,
    pub epic_summary: String,
    pub last_jira_update: Option<String>,
}

// --- Sync Report ---

#[derive(Debug, Clone, Serialize, Default)]
pub struct SyncReport {
    pub jira_to_vk: DirectionReport,
    pub vk_to_jira: DirectionReport,
    pub dry_run: bool,
    pub mappings_total: usize,
    pub epic_mappings_total: usize,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DirectionReport {
    pub created: usize,
    pub updated: usize,
    pub skipped: usize,
    pub errors: Vec<SyncError>,
    pub projects_created: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncError {
    pub key: String,
    pub error: String,
}

// --- VK API Abstraction ---

/// Trait abstracting VK API calls so the sync engine doesn't depend on McpServer directly.
#[async_trait::async_trait]
pub trait VkApi: Send + Sync {
    async fn list_issues(
        &self,
        project_id: &str,
    ) -> Result<Vec<VkIssueSummary>, String>;

    async fn create_issue(
        &self,
        project_id: &str,
        title: &str,
        description: Option<&str>,
        status: Option<&str>,
        priority: Option<&str>,
    ) -> Result<String, String>;

    /// Create an issue with a preset Jira key as simple_id.
    async fn create_issue_with_jira_key(
        &self,
        project_id: &str,
        title: &str,
        description: Option<&str>,
        status: Option<&str>,
        priority: Option<&str>,
        simple_id: &str,
        issue_number: i32,
    ) -> Result<String, String>;

    async fn update_issue(
        &self,
        issue_id: &str,
        title: Option<&str>,
        description: Option<&str>,
        status: Option<&str>,
        priority: Option<&str>,
    ) -> Result<(), String>;

    async fn assign_issue(&self, issue_id: &str, user_id: &str) -> Result<(), String>;

    async fn add_issue_tag(&self, issue_id: &str, tag_name: &str) -> Result<(), String>;

    async fn create_issue_relationship(
        &self,
        issue_id: &str,
        related_issue_id: &str,
        relationship_type: &str,
    ) -> Result<(), String>;

    async fn update_issue_parent(
        &self,
        issue_id: &str,
        parent_issue_id: &str,
    ) -> Result<(), String>;

    /// Create a new VK project under an organization.
    async fn create_project(
        &self,
        organization_id: &str,
        name: &str,
    ) -> Result<String, String>;

    /// List projects for an organization.
    async fn list_projects(
        &self,
        organization_id: &str,
    ) -> Result<Vec<VkProjectSummary>, String>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VkIssueSummary {
    pub id: String,
    pub title: String,
    pub simple_id: String,
    pub description: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub updated_at: Option<String>,
    pub assignee_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VkProjectSummary {
    pub id: String,
    pub name: String,
}

// --- Sync Configuration ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    pub jira_base_url: String,
    pub jira_email: String,
    pub jira_api_token: String,
    pub jira_project_key: String,
    /// VK organization ID (Epic → Project mapping happens within this org)
    pub organization_id: String,
    pub user_mappings: Vec<UserMapping>,
}

// --- Sync Engine ---

pub struct SyncEngine {
    jira: JiraClient,
    config: SyncConfig,
    state: SyncState,
    dry_run: bool,
}

impl SyncEngine {
    pub fn new(config: SyncConfig, state: SyncState, dry_run: bool) -> Self {
        let jira = JiraClient::new(
            &config.jira_base_url,
            &config.jira_email,
            &config.jira_api_token,
        );
        Self {
            jira,
            config,
            state,
            dry_run,
        }
    }

    pub fn state(&self) -> &SyncState {
        &self.state
    }

    pub fn into_state(mut self) -> SyncState {
        // Use Jira JQL-compatible timestamp format
        self.state.last_sync = Some(
            chrono::Utc::now()
                .format("%Y-%m-%d %H:%M")
                .to_string(),
        );
        self.state
    }

    // --- Jira → VK ---

    pub async fn sync_jira_to_vk(&mut self, vk: &dyn VkApi) -> DirectionReport {
        let mut report = DirectionReport::default();

        // Build JQL with incremental filter if last_sync is available
        let jql = match &self.state.last_sync {
            Some(last) => format!(
                "project = {} AND updated >= '{}' ORDER BY updated DESC",
                self.config.jira_project_key, last
            ),
            None => format!(
                "project = {} ORDER BY updated DESC",
                self.config.jira_project_key
            ),
        };

        let jira_issues = match self.jira.get_all_issues(&jql, None).await {
            Ok(issues) => issues,
            Err(e) => {
                report.errors.push(SyncError {
                    key: "FETCH".to_string(),
                    error: format!("Failed to fetch Jira issues: {e}"),
                });
                return report;
            }
        };

        // Phase 1: Process Epics first → create VK Projects
        for issue in &jira_issues {
            let issuetype = issue
                .fields
                .issuetype
                .as_ref()
                .map(|t| t.name.as_str())
                .unwrap_or("");

            if issuetype != "Epic" && issuetype != "에픽" {
                continue;
            }

            let key = &issue.key;
            if self.state.epic_mappings.contains_key(key) {
                continue;
            }

            let summary = issue
                .fields
                .summary
                .as_deref()
                .unwrap_or("Untitled Epic");

            if self.dry_run {
                report.projects_created += 1;
                // Still register in state for dry-run so task mapping can reference it
                self.state.epic_mappings.insert(
                    key.clone(),
                    EpicMapping {
                        vk_project_id: format!("dry-run-{}", key),
                        epic_summary: summary.to_string(),
                        last_jira_update: issue.fields.updated.clone(),
                    },
                );
                continue;
            }

            match vk
                .create_project(&self.config.organization_id, summary)
                .await
            {
                Ok(project_id) => {
                    self.state.epic_mappings.insert(
                        key.clone(),
                        EpicMapping {
                            vk_project_id: project_id,
                            epic_summary: summary.to_string(),
                            last_jira_update: issue.fields.updated.clone(),
                        },
                    );
                    report.projects_created += 1;
                }
                Err(e) => {
                    report.errors.push(SyncError {
                        key: key.clone(),
                        error: format!("Failed to create VK project for Epic: {e}"),
                    });
                }
            }
        }

        // Phase 2: Process non-Epic issues → create/update VK Issues
        for issue in &jira_issues {
            let issuetype = issue
                .fields
                .issuetype
                .as_ref()
                .map(|t| t.name.as_str())
                .unwrap_or("");

            if issuetype == "Epic" || issuetype == "에픽" {
                continue; // Epics already handled
            }

            let key = &issue.key;

            // Determine VK project from Epic parent
            let epic_key = issue
                .fields
                .parent
                .as_ref()
                .map(|p| p.key.clone());

            let vk_project_id = match &epic_key {
                Some(ek) => match self.state.epic_mappings.get(ek) {
                    Some(em) => em.vk_project_id.clone(),
                    None => {
                        report.skipped += 1;
                        continue; // Epic not mapped — skip
                    }
                },
                None => {
                    report.skipped += 1;
                    continue; // No Epic parent — skip per user decision
                }
            };

            let vk_fields = map_jira_issue_to_vk(issue, &self.config.user_mappings);
            let jira_updated = issue.fields.updated.clone();

            let mapping = self.state.mappings.get(key).cloned();

            if let Some(mapping) = mapping {
                // Existing mapping → check if Jira updated since last sync
                if mapping.last_jira_update.as_deref() == jira_updated.as_deref() {
                    report.skipped += 1;
                    continue;
                }

                if !self.dry_run {
                    if let Err(e) = vk
                        .update_issue(
                            &mapping.vk_issue_id,
                            Some(&vk_fields.title),
                            Some(&vk_fields.description),
                            Some(&vk_fields.status),
                            Some(&vk_fields.priority),
                        )
                        .await
                    {
                        report.errors.push(SyncError {
                            key: key.clone(),
                            error: e,
                        });
                        continue;
                    }
                }

                self.state.mappings.insert(
                    key.clone(),
                    IssueMapping {
                        vk_issue_id: mapping.vk_issue_id,
                        last_jira_update: jira_updated,
                        last_vk_update: Some(chrono::Utc::now().to_rfc3339()),
                    },
                );
                report.updated += 1;
            } else {
                // New issue → create in VK with Jira key as simple_id
                if self.dry_run {
                    report.created += 1;
                    continue;
                }

                let issue_number = vk_fields.issue_number.unwrap_or(0);

                match vk
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
                        // Assign user
                        if let Some(ref assignee_id) = vk_fields.assignee_vk_id {
                            let _ = vk.assign_issue(&vk_id, assignee_id).await;
                        }

                        // Add labels
                        for label in &vk_fields.labels {
                            let _ = vk.add_issue_tag(&vk_id, label).await;
                        }

                        self.state.mappings.insert(
                            key.clone(),
                            IssueMapping {
                                vk_issue_id: vk_id,
                                last_jira_update: jira_updated,
                                last_vk_update: Some(chrono::Utc::now().to_rfc3339()),
                            },
                        );
                        report.created += 1;
                    }
                    Err(e) => {
                        report.errors.push(SyncError {
                            key: key.clone(),
                            error: e,
                        });
                    }
                }
            }
        }

        report
    }

    // --- VK → Jira ---

    pub async fn sync_vk_to_jira(&mut self, vk: &dyn VkApi) -> DirectionReport {
        let mut report = DirectionReport::default();

        // Collect all project IDs from epic mappings
        let project_ids: Vec<String> = self
            .state
            .epic_mappings
            .values()
            .map(|em| em.vk_project_id.clone())
            .collect();

        if project_ids.is_empty() {
            return report;
        }

        // Gather issues from all mapped VK projects
        let mut all_vk_issues = Vec::new();
        for project_id in &project_ids {
            match vk.list_issues(project_id).await {
                Ok(issues) => all_vk_issues.extend(issues),
                Err(e) => {
                    report.errors.push(SyncError {
                        key: "FETCH".to_string(),
                        error: format!("Failed to fetch VK issues from project {project_id}: {e}"),
                    });
                }
            }
        }

        for vk_issue in &all_vk_issues {
            // The simple_id IS the Jira key for synced issues
            let jira_key = if vk_issue.simple_id.contains('-') {
                Some(vk_issue.simple_id.clone())
            } else {
                extract_jira_key(&vk_issue.title)
            };

            if let Some(jira_key) = jira_key {
                let mapping = self.state.mappings.get(&jira_key).cloned();

                if mapping.is_none() {
                    // Known by simple_id but not in mapping → register
                    self.state.mappings.insert(
                        jira_key,
                        IssueMapping {
                            vk_issue_id: vk_issue.id.clone(),
                            last_jira_update: None,
                            last_vk_update: vk_issue.updated_at.clone(),
                        },
                    );
                    report.skipped += 1;
                    continue;
                }

                let mapping = mapping.unwrap();
                if mapping.last_vk_update.as_deref() == vk_issue.updated_at.as_deref() {
                    report.skipped += 1;
                    continue;
                }

                if self.dry_run {
                    report.updated += 1;
                    continue;
                }

                // Update Jira fields
                let mut fields = serde_json::json!({"summary": &vk_issue.title});
                if let Some(ref desc) = vk_issue.description {
                    if let Some(adf) = vk_description_to_jira(Some(desc)) {
                        fields["description"] = adf;
                    }
                }
                if let Some(ref priority) = vk_issue.priority {
                    fields["priority"] =
                        serde_json::json!({"name": map_vk_priority_to_jira(priority)});
                }

                if let Err(e) = self.jira.update_issue(&jira_key, &fields).await {
                    report.errors.push(SyncError {
                        key: jira_key.clone(),
                        error: e,
                    });
                    continue;
                }

                // Transition status
                if let Some(ref status) = vk_issue.status {
                    let target = map_vk_status_to_jira(status);
                    let _ = self.jira.transition_to_status(&jira_key, target).await;
                }

                // Update assignee
                if let Some(ref assignee_id) = vk_issue.assignee_id {
                    if let Some(jira_id) =
                        map_vk_user_to_jira(assignee_id, &self.config.user_mappings)
                    {
                        let _ = self.jira.assign_issue(&jira_key, &jira_id).await;
                    }
                }

                self.state.mappings.insert(
                    jira_key,
                    IssueMapping {
                        vk_issue_id: vk_issue.id.clone(),
                        last_jira_update: None,
                        last_vk_update: vk_issue.updated_at.clone(),
                    },
                );
                report.updated += 1;
            } else {
                // No Jira key → create new Jira issue
                if self.dry_run {
                    report.created += 1;
                    continue;
                }

                let jira_fields = jira::mapper::map_vk_issue_to_jira_fields(
                    &vk_issue.title,
                    vk_issue.description.as_deref(),
                    vk_issue.priority.as_deref(),
                    &self.config.jira_project_key,
                );

                match self.jira.create_issue(&jira_fields).await {
                    Ok(created) => {
                        let new_key = &created.key;

                        // Transition status
                        if let Some(ref status) = vk_issue.status {
                            let target = map_vk_status_to_jira(status);
                            let _ = self.jira.transition_to_status(new_key, target).await;
                        }

                        self.state.mappings.insert(
                            new_key.clone(),
                            IssueMapping {
                                vk_issue_id: vk_issue.id.clone(),
                                last_jira_update: None,
                                last_vk_update: vk_issue.updated_at.clone(),
                            },
                        );
                        report.created += 1;
                    }
                    Err(e) => {
                        report.errors.push(SyncError {
                            key: vk_issue.id.clone(),
                            error: e,
                        });
                    }
                }
            }
        }

        report
    }

    // --- Full Sync ---

    pub async fn run_full_sync(&mut self, vk: &dyn VkApi) -> SyncReport {
        let jira_to_vk = self.sync_jira_to_vk(vk).await;
        let vk_to_jira = self.sync_vk_to_jira(vk).await;

        SyncReport {
            jira_to_vk,
            vk_to_jira,
            dry_run: self.dry_run,
            mappings_total: self.state.mappings.len(),
            epic_mappings_total: self.state.epic_mappings.len(),
        }
    }
}

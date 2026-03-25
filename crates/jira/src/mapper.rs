//! Field mappers between Jira and VK (vibe-kanban) data models.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adf::{adf_to_markdown, markdown_to_adf};
use crate::client::JiraIssue;

// --- Status Mapping ---

pub fn map_jira_status_to_vk(jira_status: &str) -> &'static str {
    match jira_status {
        "미해결" | "To Do" => "todo",
        "진행 중" | "In Progress" => "inprogress",
        "PUT ON HOLD" => "todo",
        "해결됨" | "Done" => "done",
        "종료" | "Closed" => "done",
        "취소" | "Cancelled" => "cancelled",
        _ => "todo",
    }
}

pub fn map_vk_status_to_jira(vk_status: &str) -> &'static str {
    let normalized: String = vk_status.to_lowercase().replace(' ', "");
    match normalized.as_str() {
        "todo" => "미해결",
        "inprogress" => "진행 중",
        "inreview" => "진행 중",
        "done" => "해결됨",
        "cancelled" => "취소",
        _ => "미해결",
    }
}

// --- Priority Mapping ---

pub fn map_jira_priority_to_vk(jira_priority: &str) -> &'static str {
    match jira_priority {
        "Highest" => "urgent",
        "High" => "high",
        "Medium" => "medium",
        "Low" => "low",
        "Lowest" => "lowest",
        _ => "medium",
    }
}

pub fn map_vk_priority_to_jira(vk_priority: &str) -> &'static str {
    match vk_priority.to_lowercase().as_str() {
        "urgent" => "Highest",
        "high" => "High",
        "medium" => "Medium",
        "low" => "Low",
        "lowest" => "Lowest",
        _ => "Medium",
    }
}

// --- User Mapping ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMapping {
    pub vk_user_id: String,
    pub jira_account_id: String,
}

pub fn map_jira_user_to_vk(jira_account_id: &str, user_map: &[UserMapping]) -> Option<String> {
    user_map
        .iter()
        .find(|u| u.jira_account_id == jira_account_id)
        .map(|u| u.vk_user_id.clone())
}

pub fn map_vk_user_to_jira(vk_user_id: &str, user_map: &[UserMapping]) -> Option<String> {
    user_map
        .iter()
        .find(|u| u.vk_user_id == vk_user_id)
        .map(|u| u.jira_account_id.clone())
}

// --- Relationship Type Mapping ---

pub fn map_jira_link_type_to_vk(jira_link_type: &str) -> &'static str {
    match jira_link_type {
        "Blocks" | "is blocked by" => "blocking",
        "Relates" => "related",
        "Duplicate" | "is duplicated by" => "has_duplicate",
        _ => "related",
    }
}

pub fn map_vk_link_type_to_jira(vk_link_type: &str) -> &'static str {
    match vk_link_type {
        "blocking" => "Blocks",
        "related" => "Relates",
        "has_duplicate" => "Duplicate",
        _ => "Relates",
    }
}

// --- Description Conversion ---

pub fn jira_description_to_vk(adf_description: Option<&Value>) -> String {
    match adf_description {
        Some(adf) => adf_to_markdown(adf),
        None => String::new(),
    }
}

pub fn vk_description_to_jira(markdown: Option<&str>) -> Option<Value> {
    match markdown {
        Some(md) if !md.is_empty() => Some(markdown_to_adf(md)),
        _ => None,
    }
}

// --- Title Helpers ---

/// Extract Jira key from a VK title like "[MPD-123] Some title" or simple_id
pub fn extract_jira_key(title: &str) -> Option<String> {
    let re = regex::Regex::new(r"\[([A-Z]+-\d+)\]").ok()?;
    re.captures(title)
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str().to_string())
}

/// Remove Jira key prefix from VK title to get clean summary
pub fn clean_vk_title(vk_title: &str) -> String {
    let re = regex::Regex::new(r"^\[[A-Z]+-\d+\]\s*").unwrap();
    re.replace(vk_title, "").to_string()
}

/// Extract the numeric part from a Jira key (e.g., "MPD-172" → 172)
pub fn extract_issue_number(jira_key: &str) -> Option<i32> {
    jira_key
        .split('-')
        .last()
        .and_then(|n| n.parse::<i32>().ok())
}

// --- Full Issue Mapping ---

/// Fields needed to create/update a VK issue from a Jira issue
#[derive(Debug, Clone)]
pub struct VkIssueFields {
    pub title: String,
    pub description: String,
    pub status: String,
    pub priority: String,
    pub simple_id: String,
    pub issue_number: Option<i32>,
    pub assignee_vk_id: Option<String>,
    pub labels: Vec<String>,
}

/// Map a Jira issue to VK create/update fields.
/// Title uses Jira summary directly (no key prefix).
/// simple_id is set to the Jira key (e.g., "MPD-172").
pub fn map_jira_issue_to_vk(jira_issue: &JiraIssue, user_map: &[UserMapping]) -> VkIssueFields {
    let fields = &jira_issue.fields;
    let summary = fields.summary.as_deref().unwrap_or("");
    let status_name = fields
        .status
        .as_ref()
        .map(|s| s.name.as_str())
        .unwrap_or("To Do");
    let priority_name = fields
        .priority
        .as_ref()
        .map(|p| p.name.as_str())
        .unwrap_or("Medium");

    let assignee_vk_id = fields
        .assignee
        .as_ref()
        .and_then(|a| map_jira_user_to_vk(&a.account_id, user_map));

    let labels = fields.labels.clone().unwrap_or_default();

    VkIssueFields {
        title: summary.to_string(),
        description: jira_description_to_vk(fields.description.as_ref()),
        status: map_jira_status_to_vk(status_name).to_string(),
        priority: map_jira_priority_to_vk(priority_name).to_string(),
        simple_id: jira_issue.key.clone(),
        issue_number: extract_issue_number(&jira_issue.key),
        assignee_vk_id,
        labels,
    }
}

/// Map a VK issue to Jira create fields
pub fn map_vk_issue_to_jira_fields(
    title: &str,
    description: Option<&str>,
    priority: Option<&str>,
    project_key: &str,
) -> Value {
    let summary = clean_vk_title(title);
    let summary = if summary.is_empty() { title } else { &summary };

    let mut fields = serde_json::json!({
        "project": {"key": project_key},
        "summary": summary,
        "issuetype": {"name": "Task"},
    });

    if let Some(desc) = vk_description_to_jira(description) {
        fields["description"] = desc;
    }

    if let Some(p) = priority {
        fields["priority"] = serde_json::json!({"name": map_vk_priority_to_jira(p)});
    }

    fields
}

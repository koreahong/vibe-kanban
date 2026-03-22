//! Jira REST API v3 client with Basic auth.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct JiraClient {
    base_url: String,
    auth_header: String,
    client: Client,
}

// --- Response types ---

#[derive(Debug, Deserialize)]
pub struct JiraSearchResponse {
    pub issues: Vec<JiraIssue>,
    #[serde(rename = "nextPageToken")]
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JiraIssue {
    pub id: String,
    pub key: String,
    #[serde(rename = "self")]
    pub self_url: Option<String>,
    pub fields: JiraIssueFields,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JiraIssueFields {
    pub summary: Option<String>,
    pub description: Option<Value>,
    pub status: Option<JiraStatus>,
    pub priority: Option<JiraPriority>,
    pub assignee: Option<JiraUser>,
    pub labels: Option<Vec<String>>,
    pub parent: Option<Box<JiraIssueRef>>,
    pub subtasks: Option<Vec<JiraIssueRef>>,
    pub issuelinks: Option<Vec<JiraIssueLink>>,
    pub updated: Option<String>,
    pub issuetype: Option<JiraIssueType>,
    pub duedate: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JiraStatus {
    pub name: String,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JiraPriority {
    pub name: String,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JiraUser {
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JiraIssueRef {
    pub id: Option<String>,
    pub key: String,
    pub fields: Option<JiraIssueRefFields>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JiraIssueRefFields {
    pub summary: Option<String>,
    pub status: Option<JiraStatus>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JiraIssueLink {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub link_type: Option<JiraLinkType>,
    #[serde(rename = "inwardIssue")]
    pub inward_issue: Option<JiraIssueRef>,
    #[serde(rename = "outwardIssue")]
    pub outward_issue: Option<JiraIssueRef>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JiraLinkType {
    pub name: String,
    pub inward: Option<String>,
    pub outward: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JiraIssueType {
    pub name: String,
    pub subtask: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct JiraTransition {
    pub id: String,
    pub name: String,
    pub to: JiraStatus,
}

#[derive(Debug, Deserialize)]
struct JiraTransitionsResponse {
    transitions: Vec<JiraTransition>,
}

#[derive(Debug, Deserialize)]
pub struct JiraCreateResponse {
    pub id: String,
    pub key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JiraComment {
    pub id: Option<String>,
    pub author: Option<JiraUser>,
    pub body: Option<Value>,
    pub created: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JiraCommentsResponse {
    comments: Vec<JiraComment>,
}

static SHARED_HTTP_CLIENT: std::sync::OnceLock<Client> = std::sync::OnceLock::new();

fn shared_client() -> &'static Client {
    SHARED_HTTP_CLIENT.get_or_init(Client::new)
}

impl JiraClient {
    pub fn new(base_url: &str, email: &str, api_token: &str) -> Self {
        let credentials = format!("{email}:{api_token}");
        let auth_header = format!("Basic {}", BASE64.encode(credentials.as_bytes()));
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            auth_header,
            client: shared_client().clone(),
        }
    }

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Option<Value>, String> {
        let url = format!("{}/rest/api/3{}", self.base_url, path);
        let mut rb = self
            .client
            .request(method, &url)
            .header("Authorization", &self.auth_header)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json");

        if let Some(body) = body {
            rb = rb.json(body);
        }

        let resp = rb.send().await.map_err(|e| format!("Jira request failed: {e}"))?;
        let status = resp.status();

        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(format!("Jira API {status}: {path}\n{body_text}"));
        }

        if status.as_u16() == 204 {
            return Ok(None);
        }

        let val: Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse Jira response: {e}"))?;
        Ok(Some(val))
    }

    // --- Issues ---

    pub async fn search_issues(
        &self,
        jql: &str,
        fields: Option<&[&str]>,
        max_results: u32,
    ) -> Result<JiraSearchResponse, String> {
        let default_fields = vec![
            "summary",
            "description",
            "status",
            "priority",
            "assignee",
            "labels",
            "parent",
            "subtasks",
            "issuelinks",
            "updated",
            "issuetype",
        ];
        let fields = fields.unwrap_or(&default_fields);

        let body = serde_json::json!({
            "jql": jql,
            "fields": fields,
            "maxResults": max_results,
        });

        let resp = self
            .request(reqwest::Method::POST, "/search/jql", Some(&body))
            .await?
            .ok_or("Empty search response")?;

        serde_json::from_value(resp).map_err(|e| format!("Failed to parse search response: {e}"))
    }

    pub async fn get_all_issues(
        &self,
        jql: &str,
        fields: Option<&[&str]>,
    ) -> Result<Vec<JiraIssue>, String> {
        let default_fields = vec![
            "summary",
            "description",
            "status",
            "priority",
            "assignee",
            "labels",
            "parent",
            "subtasks",
            "issuelinks",
            "updated",
            "issuetype",
        ];
        let fields_list = fields.unwrap_or(&default_fields);
        let mut all_issues = Vec::new();
        let mut next_page_token: Option<String> = None;
        let max_results = 100u32;

        loop {
            let mut body = serde_json::json!({
                "jql": jql,
                "fields": fields_list,
                "maxResults": max_results,
            });

            if let Some(ref token) = next_page_token {
                body["nextPageToken"] = serde_json::json!(token);
            }

            let resp = self
                .request(reqwest::Method::POST, "/search/jql", Some(&body))
                .await?
                .ok_or("Empty search response")?;

            let result: JiraSearchResponse = serde_json::from_value(resp)
                .map_err(|e| format!("Failed to parse search response: {e}"))?;

            all_issues.extend(result.issues);

            match result.next_page_token {
                Some(token) if !token.is_empty() => next_page_token = Some(token),
                _ => break,
            }
        }

        Ok(all_issues)
    }

    pub async fn get_issue(&self, issue_key: &str) -> Result<JiraIssue, String> {
        let resp = self
            .request(reqwest::Method::GET, &format!("/issue/{issue_key}"), None)
            .await?
            .ok_or("Empty issue response")?;

        serde_json::from_value(resp).map_err(|e| format!("Failed to parse issue: {e}"))
    }

    pub async fn create_issue(&self, fields: &Value) -> Result<JiraCreateResponse, String> {
        let body = serde_json::json!({ "fields": fields });
        let resp = self
            .request(reqwest::Method::POST, "/issue", Some(&body))
            .await?
            .ok_or("Empty create response")?;

        serde_json::from_value(resp).map_err(|e| format!("Failed to parse create response: {e}"))
    }

    pub async fn update_issue(&self, issue_key: &str, fields: &Value) -> Result<(), String> {
        let body = serde_json::json!({ "fields": fields });
        self.request(
            reqwest::Method::PUT,
            &format!("/issue/{issue_key}"),
            Some(&body),
        )
        .await?;
        Ok(())
    }

    // --- Transitions ---

    pub async fn get_transitions(&self, issue_key: &str) -> Result<Vec<JiraTransition>, String> {
        let resp = self
            .request(
                reqwest::Method::GET,
                &format!("/issue/{issue_key}/transitions"),
                None,
            )
            .await?
            .ok_or("Empty transitions response")?;

        let result: JiraTransitionsResponse = serde_json::from_value(resp)
            .map_err(|e| format!("Failed to parse transitions: {e}"))?;
        Ok(result.transitions)
    }

    pub async fn transition_to_status(
        &self,
        issue_key: &str,
        target_status: &str,
    ) -> Result<(), String> {
        let transitions = self.get_transitions(issue_key).await?;
        let matched = transitions
            .iter()
            .find(|t| t.to.name == target_status || t.name == target_status);

        match matched {
            Some(t) => {
                let body = serde_json::json!({
                    "transition": { "id": t.id }
                });
                self.request(
                    reqwest::Method::POST,
                    &format!("/issue/{issue_key}/transitions"),
                    Some(&body),
                )
                .await?;
                Ok(())
            }
            None => {
                let available: Vec<String> = transitions
                    .iter()
                    .map(|t| format!("{} → {}", t.name, t.to.name))
                    .collect();
                Err(format!(
                    "No transition to \"{target_status}\" for {issue_key}. Available: {}",
                    available.join(", ")
                ))
            }
        }
    }

    // --- Comments ---

    pub async fn get_issue_comments(&self, issue_key: &str) -> Result<Vec<JiraComment>, String> {
        let resp = self
            .request(
                reqwest::Method::GET,
                &format!("/issue/{issue_key}/comment"),
                None,
            )
            .await?
            .ok_or("Empty comments response")?;

        let result: JiraCommentsResponse = serde_json::from_value(resp)
            .map_err(|e| format!("Failed to parse comments: {e}"))?;
        Ok(result.comments)
    }

    // --- Assignee ---

    pub async fn assign_issue(
        &self,
        issue_key: &str,
        account_id: &str,
    ) -> Result<(), String> {
        let body = serde_json::json!({ "accountId": account_id });
        self.request(
            reqwest::Method::PUT,
            &format!("/issue/{issue_key}/assignee"),
            Some(&body),
        )
        .await?;
        Ok(())
    }
}

//! Persistent Jira configuration stored at ~/.vibe-kanban/jira-config.json

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::mapper::UserMapping;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JiraConfig {
    pub jira_base_url: String,
    pub jira_email: String,
    pub jira_api_token: String,
    #[serde(default = "default_project_key")]
    pub jira_project_key: String,
    pub organization_id: String,
    #[serde(default)]
    pub user_mappings: Vec<UserMapping>,
}

fn default_project_key() -> String {
    "MPD".to_string()
}

fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".vibe-kanban")
        .join("jira-config.json")
}

pub fn load_config() -> Option<JiraConfig> {
    let path = config_path();
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

pub fn save_config(config: &JiraConfig) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config dir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("Failed to write config: {e}"))
}

//! Qraft-specific Jira import hooks.
//! All custom logic is isolated here to minimize upstream merge conflicts.
//! jira.rs only contains thin QRAFT-CUSTOM hook calls into this module.
//!
//! Note: Uses sqlx::query() (runtime, non-macro) instead of sqlx::query!() (compile-time)
//! to avoid needing offline query generation for new queries.

use std::collections::HashMap;

use api_types::ProjectStatus;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Duplicate import check — queries extension_metadata.jira_key in issues table.
/// Returns Err if the jira_key was already imported into this project.
pub async fn check_duplicate(
    pool: &PgPool,
    project_id: Uuid,
    jira_key: &str,
    prevent: bool,
) -> Result<(), String> {
    if !prevent {
        return Ok(());
    }
    let row = sqlx::query(
        "SELECT id FROM issues WHERE project_id = $1 AND extension_metadata->>'jira_key' = $2 LIMIT 1"
    )
    .bind(project_id)
    .bind(jira_key)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Duplicate check failed: {e}"))?;

    if row.is_some() {
        return Err(format!("Jira issue {} already imported into this project", jira_key));
    }
    Ok(())
}

/// Builds extension_metadata JSON containing the jira_key for later duplicate detection.
pub fn build_extension_metadata(jira_key: &str) -> serde_json::Value {
    serde_json::json!({"jira_key": jira_key})
}

/// Overwrites the DB-trigger-generated simple_id (e.g. EDA-3) with the Jira key (e.g. MPD-179).
/// No-op if preserve is false.
pub async fn override_simple_id(
    pool: &PgPool,
    issue_id: Uuid,
    jira_key: &str,
    preserve: bool,
) -> Result<(), String> {
    if !preserve {
        return Ok(());
    }
    sqlx::query("UPDATE issues SET simple_id = $1 WHERE id = $2")
        .bind(jira_key)
        .bind(issue_id)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to set simple_id to {}: {}", jira_key, e))?;
    Ok(())
}

/// Status resolution with two improvements over the original resolve_status_id:
/// 1. Checks config_mappings (Jira status name → VK project status name) first.
/// 2. Iterates candidates first (not statuses) so priority order is guaranteed.
///    (Original bug: statuses ordered by sort_order, so "Backlog" could match before "To do".)
pub fn resolve_status_with_config(
    statuses: &[ProjectStatus],
    vk_status: &str,
    jira_status_name: Option<&str>,
    config_mappings: &HashMap<String, String>,
) -> Option<Uuid> {
    // 1. Config direct mapping (Jira status name → VK project status name)
    if let Some(jira_name) = jira_status_name {
        if let Some(mapped_name) = config_mappings.get(jira_name) {
            if let Some(s) = statuses.iter().find(|s| s.name.eq_ignore_ascii_case(mapped_name)) {
                return Some(s.id);
            }
        }
    }

    // 2. Default mapping — iterate candidates first to guarantee priority
    let candidates: &[&str] = match vk_status {
        "todo" => &["To do", "To Do", "Backlog"],
        "inprogress" => &["In progress", "In Progress"],
        "done" => &["Done"],
        "cancelled" => &["Cancelled", "Canceled"],
        _ => &["To do", "To Do", "Backlog"],
    };

    candidates
        .iter()
        .find_map(|n| statuses.iter().find(|s| s.name.eq_ignore_ascii_case(n)))
        .or_else(|| statuses.iter().find(|s| !s.hidden))
        .or_else(|| statuses.first())
        .map(|s| s.id)
}

// QRAFT-CUSTOM: Jira status sync on card move

/// VK project status name → Jira status name.
/// config_mappings is Jira→VK direction, so we reverse-lookup first.
fn resolve_jira_target_status(vk_status_name: &str, config_mappings: &HashMap<String, String>) -> String {
    // 1. config reverse lookup (VK name → Jira name)
    for (jira_name, vk_name) in config_mappings {
        if vk_name.eq_ignore_ascii_case(vk_status_name) {
            return jira_name.clone();
        }
    }
    // 2. default mapping by VK status name pattern
    let lower = vk_status_name.to_lowercase();
    if lower.contains("done") {
        "해결됨".to_string()
    } else if lower.contains("review") || lower.contains("progress") {
        "진행 중".to_string()
    } else if lower.contains("cancel") {
        "취소".to_string()
    } else if lower.contains("hold") {
        "PUT ON HOLD".to_string()
    } else {
        "미해결".to_string()
    }
}

/// Sync VK issue status change back to Jira via transition.
/// Only acts on issues with extension_metadata.jira_key set (i.e., Jira-imported issues).
/// Silently no-ops if Jira is not configured or jira_key is absent.
pub async fn sync_status_to_jira(
    extension_metadata: &serde_json::Value,
    new_status_name: &str,
    config_mappings: &HashMap<String, String>,
) {
    // 1. Extract jira_key from extension_metadata
    let jira_key = match extension_metadata.get("jira_key").and_then(|v| v.as_str()) {
        Some(k) => k.to_string(),
        None => return,
    };

    // 2. Load Jira config
    let cfg = match jira::config::load_config() {
        Some(c) => c,
        None => return,
    };

    // 3. Resolve target Jira status name
    let target_status = resolve_jira_target_status(new_status_name, config_mappings);

    // 4. Execute transition
    let client = jira::client::JiraClient::new(&cfg.jira_base_url, &cfg.jira_email, &cfg.jira_api_token);
    if let Err(e) = client.transition_to_status(&jira_key, &target_status).await {
        tracing::warn!(
            jira_key = %jira_key,
            target_status = %target_status,
            error = %e,
            "Jira status sync failed (non-fatal)"
        );
    } else {
        tracing::info!(jira_key = %jira_key, target_status = %target_status, "Jira status synced");
    }
}

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

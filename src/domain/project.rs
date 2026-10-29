//! Project domain type.

use super::{Id, Timestamp};
use serde::{Deserialize, Serialize};

/// Owns a DSN, retention setting, mute flag; container for issues.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Id,
    /// Short, URL-friendly public code (6 chars, `a-z0-9`); used in web URLs and
    /// the SPA-facing API instead of the verbose UUID. The UUID [`id`](Self::id)
    /// remains the internal key (foreign keys, DSN, ingestion).
    pub short_id: String,
    pub team_id: Id,
    pub name: String,
    pub slug: String,
    /// DSN public key used for ingestion auth.
    pub dsn_public_key: String,
    /// Max stored events for this project (overrides `DEFAULT_EVENTS_RETENTION`).
    pub retention_events: i64,
    /// Age-based retention in days; 0 disables age-based pruning.
    /// Overrides `DEFAULT_RETENTION_DAYS` when > 0.
    pub retention_days: i64,
    /// Project-level mute: suppresses notifications; ingestion continues.
    pub muted: bool,
    /// Optional per-project webhook URL for new-issue/regression notifications
    ///. When set, notifications are POSTed as JSON under the same
    /// suppression rules as email.
    pub webhook_url: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub orphaned_at: Option<Timestamp>,
}

pub fn slugify(input: &str) -> String {
    let mut slug = String::with_capacity(input.len());
    let mut prev_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
    }
    let trimmed = slug.trim_matches('-').to_string();
    if trimmed.is_empty() {
        // Fall back to a random suffix so slugs are never empty.
        return format!("project-{}", &Id::new_v4().simple().to_string()[..8]);
    }
    trimmed
}

pub fn is_valid_webhook_url(value: &str) -> bool {
    match reqwest::Url::parse(value) {
        Ok(url) => matches!(url.scheme(), "http" | "https"),
        Err(_) => false,
    }
}

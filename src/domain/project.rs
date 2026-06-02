//! Project domain type.

use super::{Id, Timestamp};
use serde::{Deserialize, Serialize};

/// Owns a DSN, retention setting, mute flag; container for issues.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Id,
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
}

//! Project domain type (MVP §8.2).

use super::{Id, Timestamp};
use serde::{Deserialize, Serialize};

/// Owns a DSN, retention setting, mute flag; container for issues.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Id,
    pub team_id: Id,
    pub name: String,
    pub slug: String,
    /// DSN public key used for ingestion auth (§5.1).
    pub dsn_public_key: String,
    /// Max stored events for this project (overrides `DEFAULT_EVENTS_RETENTION`).
    pub retention_events: i64,
    /// Project-level mute: suppresses notifications; ingestion continues (§8.2).
    pub muted: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

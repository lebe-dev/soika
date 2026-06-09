//! Issue domain type and status.

use super::{Id, Timestamp};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Issue lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueStatus {
    Unresolved,
    Resolved,
    /// Issue-level mute: events still accepted/counted, no notifications.
    Muted,
}

impl IssueStatus {
    /// Stable string form persisted in the database.
    pub fn as_str(&self) -> &'static str {
        match self {
            IssueStatus::Unresolved => "unresolved",
            IssueStatus::Resolved => "resolved",
            IssueStatus::Muted => "muted",
        }
    }
}

impl fmt::Display for IssueStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for IssueStatus {
    type Err = crate::error::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "unresolved" => Ok(IssueStatus::Unresolved),
            "resolved" => Ok(IssueStatus::Resolved),
            "muted" => Ok(IssueStatus::Muted),
            other => Err(crate::error::Error::validation(format!(
                "invalid issue status: {other}"
            ))),
        }
    }
}

/// A group of events sharing a fingerprint; carries status & counters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub id: Id,
    /// Short, URL-friendly public code (6 chars, `a-z0-9`); used in web URLs and
    /// the SPA-facing API instead of the verbose UUID. The UUID [`id`](Self::id)
    /// remains the internal key.
    pub short_id: String,
    pub project_id: Id,
    pub fingerprint: String,
    pub title: String,
    pub culprit: Option<String>,
    pub level: Option<String>,
    /// Sentry top-level `environment` (e.g. `production`, `staging`), if any.
    pub environment: Option<String>,
    /// Sentry top-level `release` (version/build identifier), if any.
    pub release: Option<String>,
    pub status: IssueStatus,
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
    pub event_count: i64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

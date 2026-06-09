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
    /// When the current mute was applied. `None` unless [`status`](Self::status)
    /// is [`IssueStatus::Muted`]; baseline for the event-rate window.
    pub muted_at: Option<Timestamp>,
    /// Time-based mute expiry: the scheduler auto-unmutes once this passes.
    /// `None` for an indefinite ("forever") or event-rate mute.
    pub muted_until: Option<Timestamp>,
    /// Event-rate mute: number of events within
    /// [`mute_window_seconds`](Self::mute_window_seconds) that auto-resurfaces
    /// the issue. `None` for an indefinite or time-based mute.
    pub mute_threshold: Option<i64>,
    /// Rolling window (seconds) paired with [`mute_threshold`](Self::mute_threshold).
    pub mute_window_seconds: Option<i64>,
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
    pub event_count: i64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// How long / under what condition an issue should stay muted.
///
/// Built by the API from the chosen dropdown period and persisted onto the
/// issue's mute columns by [`IssueRepository::mute`](crate::ports::IssueRepository::mute).
/// The three shapes are mutually exclusive; the default ([`MuteSpec::FOREVER`])
/// is an indefinite mute that only ends when the user unmutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MuteSpec {
    /// Mute indefinitely — no expiry, no auto-resurface.
    Forever,
    /// Auto-unmute once `until` passes (time-based; cleared by the scheduler).
    Until(Timestamp),
    /// Auto-unmute once `threshold` events arrive within `window_seconds`
    /// (event-rate; cleared by ingestion).
    EventRate { threshold: i64, window_seconds: i64 },
}

impl MuteSpec {
    /// The default indefinite mute (mirrors the bare "Mute" button).
    pub const FOREVER: MuteSpec = MuteSpec::Forever;
}

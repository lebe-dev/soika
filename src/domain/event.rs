//! Event domain type (MVP §5) — a single captured occurrence.

use super::{Id, Timestamp};
use serde::{Deserialize, Serialize};

/// An individual error/message occurrence with its full payload as received.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Internal id.
    pub id: Id,
    /// Sentry `event_id` (hex32) from the SDK.
    pub event_id: String,
    pub issue_id: Id,
    pub project_id: Id,
    /// Full event JSON exactly as received (stacktrace, tags, context, etc.).
    pub payload: serde_json::Value,
    pub received_at: Timestamp,
}

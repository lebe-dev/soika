//! Event repository port.

use crate::domain::{Event, Id, Timestamp};
use crate::error::Result;
use async_trait::async_trait;

/// Data required to insert a captured event.
#[derive(Debug, Clone)]
pub struct NewEvent {
    pub event_id: String,
    pub issue_id: Id,
    pub project_id: Id,
    pub payload: serde_json::Value,
    pub received_at: Timestamp,
}

/// CRUD + retention queries for events.
#[async_trait]
pub trait EventRepository: Send + Sync {
    async fn insert(&self, new: NewEvent) -> Result<Event>;
    async fn find_by_id(&self, id: Id) -> Result<Option<Event>>;

    /// Most recent events for an issue (newest first), for the issue detail view.
    async fn recent_events(&self, issue_id: Id, limit: i64) -> Result<Vec<Event>>;

    /// The latest single event for an issue (issue detail default view).
    async fn latest_for_issue(&self, issue_id: Id) -> Result<Option<Event>>;

    /// Count of events currently stored for a project.
    async fn count_for_project(&self, project_id: Id) -> Result<i64>;

    /// Retention: keep at most `retention_events` most-recent events for the
    /// project, deleting older ones. Returns number of rows deleted.
    async fn prune_events_over_retention(
        &self,
        project_id: Id,
        retention_events: i64,
    ) -> Result<u64>;

    /// All distinct project ids that currently have events (for the cron sweep).
    async fn project_ids_with_events(&self) -> Result<Vec<Id>>;

    /// Delete events older than `cutoff` (optional age-based cleanup).
    async fn delete_older_than(&self, project_id: Id, cutoff: Timestamp) -> Result<u64>;
}

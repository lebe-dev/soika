//! Issue repository port (MVP §7, §8.1).

use crate::domain::{Id, Issue, IssueStatus, Timestamp};
use crate::error::Result;
use async_trait::async_trait;

/// Fields used to create-or-update an issue by fingerprint during ingestion.
#[derive(Debug, Clone)]
pub struct IssueUpsert {
    pub project_id: Id,
    pub fingerprint: String,
    pub title: String,
    pub culprit: Option<String>,
    pub level: Option<String>,
    /// Time of the event driving this upsert (becomes first/last_seen).
    pub seen_at: Timestamp,
}

/// The outcome of an upsert, so ingestion can detect new issues & regressions.
#[derive(Debug, Clone)]
pub struct UpsertOutcome {
    pub issue: Issue,
    /// True if this fingerprint was seen for the first time (→ new-issue notice).
    pub is_new: bool,
    /// True if a `resolved` issue transitioned back to `unresolved` (→ regression).
    pub is_regression: bool,
}

/// Filter for listing issues (MVP §8 issues list).
#[derive(Debug, Clone, Default)]
pub struct IssueFilter {
    pub status: Option<IssueStatus>,
    pub query: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// CRUD + grouping/ingestion queries for issues.
#[async_trait]
pub trait IssueRepository: Send + Sync {
    async fn find_by_id(&self, id: Id) -> Result<Option<Issue>>;
    async fn find_by_fingerprint(&self, project_id: Id, fingerprint: &str)
        -> Result<Option<Issue>>;

    /// Create-or-update an issue for a fingerprint, applying regression logic
    /// (resolved → unresolved on a new event). Bumps `last_seen` (§7, §8.1).
    async fn upsert_by_fingerprint(&self, upsert: IssueUpsert) -> Result<UpsertOutcome>;

    /// Set issue status (resolve / mute / unresolve — §8.1).
    async fn set_status(&self, issue_id: Id, status: IssueStatus) -> Result<Issue>;

    /// List issues in a project with filtering (§8 issues list).
    async fn list(&self, project_id: Id, filter: IssueFilter) -> Result<Vec<Issue>>;

    async fn delete(&self, id: Id) -> Result<()>;
}

//! Issue repository port.

use crate::domain::{Id, Issue, IssueStatus, MuteSpec, Timestamp};
use crate::error::Result;
use async_trait::async_trait;
use std::collections::HashMap;

/// Fields used to create-or-update an issue by fingerprint during ingestion.
#[derive(Debug, Clone)]
pub struct IssueUpsert {
    pub project_id: Id,
    pub fingerprint: String,
    pub title: String,
    pub culprit: Option<String>,
    pub level: Option<String>,
    /// Sentry top-level `environment` (set on first sight, like culprit/level).
    pub environment: Option<String>,
    /// Sentry top-level `release` (set on first sight, like culprit/level).
    pub release: Option<String>,
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

/// Ordering for the issues list (frequency/recency sort).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IssueSort {
    /// Most recently seen first (`last_seen DESC`). The default ordering.
    #[default]
    LastSeen,
    /// Most frequent first (`event_count DESC`, `last_seen DESC` tie-break).
    EventCount,
}

/// Filter for listing issues (issues list).
///
/// `status`, `level`, `environment` and `release` are exact-match equality
/// filters; `None` matches all (Stories 4.4 / 5.2). `query` is a substring
/// match on title/culprit.
#[derive(Debug, Clone, Default)]
pub struct IssueFilter {
    pub status: Option<IssueStatus>,
    /// Exact-match severity level (`error`, `warning`, ...); free-form per Sentry.
    pub level: Option<String>,
    /// Exact-match environment (`production`, `staging`, ...).
    pub environment: Option<String>,
    /// Exact-match release (version/build identifier).
    pub release: Option<String>,
    pub query: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    /// Result ordering; `Default` yields [`IssueSort::LastSeen`].
    pub sort: IssueSort,
}

/// CRUD + grouping/ingestion queries for issues.
#[async_trait]
pub trait IssueRepository: Send + Sync {
    async fn find_by_id(&self, id: Id) -> Result<Option<Issue>>;

    /// Resolve an issue from its short public id (web URLs / SPA API).
    async fn find_by_short_id(&self, short_id: &str) -> Result<Option<Issue>>;

    async fn find_by_fingerprint(&self, project_id: Id, fingerprint: &str)
    -> Result<Option<Issue>>;

    /// Create-or-update an issue for a fingerprint, applying regression logic
    /// (resolved → unresolved on a new event). Bumps `last_seen`.
    async fn upsert_by_fingerprint(&self, upsert: IssueUpsert) -> Result<UpsertOutcome>;

    /// Set issue status (resolve / unresolve).
    ///
    /// Transitioning to any non-muted status clears the mute bookkeeping
    /// (`muted_at` / `muted_until` / `mute_threshold` / `mute_window_seconds`),
    /// so re-opening or resolving a muted issue never leaves a stale expiry
    /// behind. To mute, use [`mute`](Self::mute) instead.
    async fn set_status(&self, issue_id: Id, status: IssueStatus) -> Result<Issue>;

    /// Mute an issue under `spec`, recording `now` as the mute baseline.
    ///
    /// Sets the status to [`IssueStatus::Muted`] and persists the period/rate
    /// columns from `spec`; a [`MuteSpec::Forever`] leaves them all `NULL`.
    async fn mute(&self, issue_id: Id, spec: MuteSpec, now: Timestamp) -> Result<Issue>;

    /// Auto-unmute every issue whose time-based mute (`muted_until`) has passed
    /// `now`, flipping it back to [`IssueStatus::Unresolved`] and clearing its
    /// mute columns. Returns the number of issues unmuted. Driven by the
    /// scheduler; event-rate mutes are cleared by ingestion instead.
    async fn clear_expired_mutes(&self, now: Timestamp) -> Result<u64>;

    /// List issues in a project with filtering (issues list).
    async fn list(&self, project_id: Id, filter: IssueFilter) -> Result<Vec<Issue>>;

    /// Count unresolved issues per project in a single query.
    ///
    /// Returns a map keyed by project id; projects with no unresolved issues
    /// are omitted (callers treat a missing key as `0`). Lets the dashboard
    /// bootstrap fetch every project's open-issue count at once instead of an
    /// N+1 of [`list`](Self::list) calls.
    async fn unresolved_counts(&self, project_ids: &[Id]) -> Result<HashMap<Id, i64>>;

    /// Latest `last_seen` timestamp of any unresolved issue per project.
    ///
    /// Returns a map keyed by project id; projects with no unresolved issues
    /// are omitted. Used by the dashboard to sort projects by most recent
    /// activity.
    async fn last_seen_per_project(&self, project_ids: &[Id]) -> Result<HashMap<Id, Timestamp>>;

    /// Override an issue's fingerprint after the fact (operator merge / split).
    ///
    /// Semantics (one transaction, respecting the unique
    /// `(project_id, fingerprint)` index):
    ///   * If no other issue in the same project already holds
    ///     `new_fingerprint`, simply rename this issue's fingerprint (split).
    ///   * If another issue (the *source*) already holds `new_fingerprint`,
    ///     MERGE into it: re-point the source's events onto the surviving
    ///     issue, fold aggregates (sum `event_count`, keep the earliest
    ///     `first_seen` and the latest `last_seen`), delete the now-empty
    ///     source issue, and return the surviving issue.
    ///
    /// Returns the surviving issue (its id may differ from `issue_id` only in
    /// the sense that the *other* row is the one deleted — here the target
    /// passed by id always survives).
    async fn override_fingerprint(&self, issue_id: Id, new_fingerprint: String) -> Result<Issue>;

    async fn delete(&self, id: Id) -> Result<()>;
}

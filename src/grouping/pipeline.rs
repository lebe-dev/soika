//! Ingest-pipeline tail (MVP §5.4) — everything after the stacktrace has been
//! normalized:
//!
//! ```text
//! compute fingerprint → upsert Issue → insert Event
//!     → update counters (first/last_seen, count)
//!     → enqueue notification check (new issue / regression)
//! ```
//!
//! Written against the repository TRAITS (`IssueRepository`, `EventRepository`)
//! plus a [`Clock`], so it is fully unit-testable with in-memory fakes and has
//! no dependency on `AppState` or the database (hexagonal architecture). The
//! `AppState`-aware ingest handler computes a [`NormalizedEvent`], calls
//! [`ingest_normalized`], then fires the notification described by the returned
//! [`IngestOutcome::notify`] through the notify port.

use crate::domain::stacktrace::NormalizedEvent;
use crate::domain::{Event, Id, Issue, Timestamp};
use crate::error::Result;
use crate::ports::{EventRepository, IssueRepository, IssueUpsert, NewEvent};

use super::{culprit_from_normalized, fingerprint_normalized, title_from_normalized};

/// Which notification (if any) the pipeline determined should be sent (§12).
///
/// Returned to the caller rather than dispatched here so the pipeline stays
/// free of `AppState` / the notify orchestration and remains unit-testable. The
/// caller is responsible for applying mute / per-user opt-out rules (§12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyKind {
    /// No notification is warranted (recurring event on a live issue).
    None,
    /// First time this fingerprint was seen in the project (§12 trigger 1).
    NewIssue,
    /// A `resolved` issue received a new event and was reopened (§12 trigger 2).
    Regression,
}

/// Inputs to the pipeline tail, gathered after parse + normalize.
#[derive(Debug, Clone)]
pub struct IngestInput {
    /// Project the DSN resolved to.
    pub project_id: Id,
    /// Sentry `event_id` (hex32) from the SDK.
    pub event_id: String,
    /// The normalized view used for grouping/title/culprit.
    pub normalized: NormalizedEvent,
    /// Full event JSON exactly as received — stored verbatim (§6).
    pub payload: serde_json::Value,
}

/// The result of running the pipeline tail for one event.
#[derive(Debug, Clone)]
pub struct IngestOutcome {
    /// The issue this event was grouped into (after any status transition).
    pub issue: Issue,
    /// The persisted event row.
    pub event: Event,
    /// The fingerprint this event grouped under.
    pub fingerprint: String,
    /// Which notification check the caller should enqueue (§5.4, §12).
    pub notify: NotifyKind,
}

/// Run the pipeline tail for a single normalized event (§5.4).
///
/// Steps: compute fingerprint → upsert issue (applying regression logic) →
/// insert event → bump counters → decide which notification to enqueue.
///
/// `seen_at` is the ingestion timestamp (the caller supplies it from the
/// [`crate::ports::Clock`]); it drives `first_seen`/`last_seen`.
pub async fn ingest_normalized(
    issues: &dyn IssueRepository,
    events: &dyn EventRepository,
    input: IngestInput,
    seen_at: Timestamp,
) -> Result<IngestOutcome> {
    let fingerprint = fingerprint_normalized(&input.normalized);
    let title = title_from_normalized(&input.normalized);
    let culprit = culprit_from_normalized(&input.normalized);
    let level = input.normalized.level.clone();
    // environment/release are top-level Sentry fields not carried on
    // NormalizedEvent, so read them straight from the verbatim payload
    // (Stories 4.4 / 5.2). Trim empties to match the `string_field` convention.
    let environment = payload_string(&input.payload, "environment");
    let release = payload_string(&input.payload, "release");

    // 1. Upsert the issue by (project_id, fingerprint). This is the SINGLE
    //    source of counter truth: the repository bumps `event_count` (+1) and
    //    advances `last_seen` for both new and existing issues, applies the
    //    regression logic (resolved → unresolved), and reports new-issue /
    //    regression via the outcome (§7, §8.1). We must NOT bump the counter a
    //    second time here, or every event would double-count (§7 counters).
    let upsert = issues
        .upsert_by_fingerprint(IssueUpsert {
            project_id: input.project_id,
            fingerprint: fingerprint.clone(),
            title,
            culprit,
            level,
            environment,
            release,
            seen_at,
        })
        .await?;

    // 2. Insert the event verbatim (§6).
    let event = events
        .insert(NewEvent {
            event_id: input.event_id,
            issue_id: upsert.issue.id,
            project_id: input.project_id,
            payload: input.payload,
            received_at: seen_at,
        })
        .await?;

    // 3. Decide which notification to enqueue. Regression takes precedence over
    //    new-issue (a brand-new issue cannot also be a regression, but guard
    //    explicitly). Mute / per-user opt-out are applied by the caller (§12).
    let notify = notify_kind(&upsert);

    // The upserted issue already reflects the bumped count + advanced last_seen,
    // so callers see fresh stats without an extra read.
    Ok(IngestOutcome {
        issue: upsert.issue,
        event,
        fingerprint,
        notify,
    })
}

/// Map an [`crate::ports::UpsertOutcome`] to the notification to enqueue.
fn notify_kind(upsert: &crate::ports::UpsertOutcome) -> NotifyKind {
    if upsert.is_regression {
        return NotifyKind::Regression;
    }
    if upsert.is_new {
        return NotifyKind::NewIssue;
    }
    NotifyKind::None
}

/// Read a top-level string field from the raw event payload, trimming and
/// dropping empties (mirrors the `string_field` convention in `stacktrace.rs`).
fn payload_string(payload: &serde_json::Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::IssueStatus;
    use crate::error::Error;
    use crate::ports::{IssueFilter, UpsertOutcome};
    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};
    use serde_json::json;
    use std::sync::Mutex;

    // --- In-memory fakes -----------------------------------------------------

    #[derive(Default)]
    struct FakeIssues {
        rows: Mutex<Vec<Issue>>,
    }

    fn base_issue(upsert: &IssueUpsert) -> Issue {
        Issue {
            id: Id::new_v4(),
            project_id: upsert.project_id,
            fingerprint: upsert.fingerprint.clone(),
            title: upsert.title.clone(),
            culprit: upsert.culprit.clone(),
            level: upsert.level.clone(),
            environment: upsert.environment.clone(),
            release: upsert.release.clone(),
            status: IssueStatus::Unresolved,
            first_seen: upsert.seen_at,
            last_seen: upsert.seen_at,
            // The real adapter inserts a brand-new issue with event_count = 1
            // (the event that created it); the fake must match that contract so
            // the pipeline test guards real behaviour.
            event_count: 1,
            created_at: upsert.seen_at,
            updated_at: upsert.seen_at,
        }
    }

    #[async_trait]
    impl IssueRepository for FakeIssues {
        async fn find_by_id(&self, id: Id) -> Result<Option<Issue>> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .iter()
                .find(|i| i.id == id)
                .cloned())
        }

        async fn find_by_fingerprint(
            &self,
            project_id: Id,
            fingerprint: &str,
        ) -> Result<Option<Issue>> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .iter()
                .find(|i| i.project_id == project_id && i.fingerprint == fingerprint)
                .cloned())
        }

        async fn upsert_by_fingerprint(&self, upsert: IssueUpsert) -> Result<UpsertOutcome> {
            let mut rows = self.rows.lock().unwrap();
            if let Some(issue) = rows
                .iter_mut()
                .find(|i| i.project_id == upsert.project_id && i.fingerprint == upsert.fingerprint)
            {
                let is_regression = issue.status == IssueStatus::Resolved;
                if is_regression {
                    issue.status = IssueStatus::Unresolved;
                }
                // Mirror the real adapter: upsert bumps the counter and advances
                // last_seen — it is the single source of counter truth.
                issue.event_count += 1;
                if upsert.seen_at > issue.last_seen {
                    issue.last_seen = upsert.seen_at;
                }
                issue.updated_at = upsert.seen_at;
                return Ok(UpsertOutcome {
                    issue: issue.clone(),
                    is_new: false,
                    is_regression,
                });
            }

            let issue = base_issue(&upsert);
            rows.push(issue.clone());
            Ok(UpsertOutcome {
                issue,
                is_new: true,
                is_regression: false,
            })
        }

        async fn set_status(&self, issue_id: Id, status: IssueStatus) -> Result<Issue> {
            let mut rows = self.rows.lock().unwrap();
            let issue = rows
                .iter_mut()
                .find(|i| i.id == issue_id)
                .ok_or_else(|| Error::not_found("issue"))?;
            issue.status = status;
            Ok(issue.clone())
        }

        async fn list(&self, _project_id: Id, _filter: IssueFilter) -> Result<Vec<Issue>> {
            Ok(self.rows.lock().unwrap().clone())
        }

        async fn override_fingerprint(
            &self,
            issue_id: Id,
            new_fingerprint: String,
        ) -> Result<Issue> {
            let mut rows = self.rows.lock().unwrap();
            // Snapshot the target's identifying fields before mutating to avoid
            // overlapping borrows.
            let (project_id, current_fp) = rows
                .iter()
                .find(|i| i.id == issue_id)
                .map(|i| (i.project_id, i.fingerprint.clone()))
                .ok_or_else(|| Error::not_found("issue"))?;

            if current_fp == new_fingerprint {
                let issue = rows.iter().find(|i| i.id == issue_id).unwrap().clone();
                return Ok(issue);
            }

            // Collision in the same project → merge source into target.
            if let Some(src_idx) = rows.iter().position(|i| {
                i.project_id == project_id && i.fingerprint == new_fingerprint && i.id != issue_id
            }) {
                let source = rows.remove(src_idx);
                let target = rows.iter_mut().find(|i| i.id == issue_id).unwrap();
                target.event_count += source.event_count;
                target.first_seen = target.first_seen.min(source.first_seen);
                target.last_seen = target.last_seen.max(source.last_seen);
                target.fingerprint = new_fingerprint;
                return Ok(target.clone());
            }

            let target = rows.iter_mut().find(|i| i.id == issue_id).unwrap();
            target.fingerprint = new_fingerprint;
            Ok(target.clone())
        }

        async fn delete(&self, id: Id) -> Result<()> {
            self.rows.lock().unwrap().retain(|i| i.id != id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeEvents {
        rows: Mutex<Vec<Event>>,
    }

    #[async_trait]
    impl EventRepository for FakeEvents {
        async fn insert(&self, new: NewEvent) -> Result<Event> {
            let event = Event {
                id: Id::new_v4(),
                event_id: new.event_id,
                issue_id: new.issue_id,
                project_id: new.project_id,
                payload: new.payload,
                received_at: new.received_at,
            };
            self.rows.lock().unwrap().push(event.clone());
            Ok(event)
        }

        async fn find_by_id(&self, id: Id) -> Result<Option<Event>> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .iter()
                .find(|e| e.id == id)
                .cloned())
        }

        async fn recent_events(&self, issue_id: Id, limit: i64) -> Result<Vec<Event>> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.issue_id == issue_id)
                .take(limit as usize)
                .cloned()
                .collect())
        }

        async fn latest_for_issue(&self, issue_id: Id) -> Result<Option<Event>> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .iter()
                .rfind(|e| e.issue_id == issue_id)
                .cloned())
        }

        async fn count_for_project(&self, project_id: Id) -> Result<i64> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.project_id == project_id)
                .count() as i64)
        }

        async fn prune_events_over_retention(
            &self,
            _project_id: Id,
            _retention_events: i64,
        ) -> Result<u64> {
            Ok(0)
        }

        async fn project_ids_with_events(&self) -> Result<Vec<Id>> {
            Ok(Vec::new())
        }

        async fn delete_older_than(&self, _project_id: Id, _cutoff: Timestamp) -> Result<u64> {
            Ok(0)
        }
    }

    fn ts(secs: i64) -> Timestamp {
        Utc.timestamp_opt(secs, 0).single().unwrap()
    }

    fn input(project_id: Id, event: serde_json::Value) -> IngestInput {
        IngestInput {
            project_id,
            event_id: "deadbeefdeadbeefdeadbeefdeadbeef".into(),
            normalized: NormalizedEvent::from_value(&event),
            payload: event,
        }
    }

    fn exc(ty: &str) -> serde_json::Value {
        json!({
            "exception": { "values": [{
                "type": ty,
                "value": "boom",
                "stacktrace": { "frames": [
                    { "function": "handle", "module": "app", "in_app": true }
                ]}
            }]}
        })
    }

    /// Like [`exc`] but injects the top-level Sentry `environment`/`release`
    /// fields the pipeline reads from the raw payload.
    fn exc_with_env(ty: &str, environment: &str, release: &str) -> serde_json::Value {
        let mut value = exc(ty);
        let map = value.as_object_mut().unwrap();
        map.insert("environment".into(), json!(environment));
        map.insert("release".into(), json!(release));
        value
    }

    #[tokio::test]
    async fn first_event_creates_issue_and_flags_new() {
        let issues = FakeIssues::default();
        let events = FakeEvents::default();
        let project = Id::new_v4();

        let out = ingest_normalized(&issues, &events, input(project, exc("ValueError")), ts(100))
            .await
            .unwrap();

        assert_eq!(out.notify, NotifyKind::NewIssue);
        assert_eq!(out.issue.event_count, 1);
        assert_eq!(out.issue.status, IssueStatus::Unresolved);
        assert_eq!(events.count_for_project(project).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn populates_environment_and_release() {
        let issues = FakeIssues::default();
        let events = FakeEvents::default();
        let project = Id::new_v4();

        let out = ingest_normalized(
            &issues,
            &events,
            input(project, exc_with_env("ValueError", "prod", "1.2.3")),
            ts(100),
        )
        .await
        .unwrap();

        assert_eq!(out.issue.environment.as_deref(), Some("prod"));
        assert_eq!(out.issue.release.as_deref(), Some("1.2.3"));
    }

    #[tokio::test]
    async fn missing_environment_and_release_default_to_none() {
        let issues = FakeIssues::default();
        let events = FakeEvents::default();
        let project = Id::new_v4();

        let out = ingest_normalized(&issues, &events, input(project, exc("ValueError")), ts(100))
            .await
            .unwrap();

        assert_eq!(out.issue.environment, None);
        assert_eq!(out.issue.release, None);
    }

    #[tokio::test]
    async fn repeated_event_groups_and_does_not_notify() {
        let issues = FakeIssues::default();
        let events = FakeEvents::default();
        let project = Id::new_v4();

        let first = ingest_normalized(&issues, &events, input(project, exc("ValueError")), ts(100))
            .await
            .unwrap();
        let second =
            ingest_normalized(&issues, &events, input(project, exc("ValueError")), ts(200))
                .await
                .unwrap();

        assert_eq!(first.issue.id, second.issue.id, "same fingerprint groups");
        assert_eq!(second.notify, NotifyKind::None);
        assert_eq!(second.issue.event_count, 2);
        assert_eq!(second.issue.last_seen, ts(200));
        assert_eq!(issues.rows.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn distinct_fingerprints_create_distinct_issues() {
        let issues = FakeIssues::default();
        let events = FakeEvents::default();
        let project = Id::new_v4();

        let a = ingest_normalized(&issues, &events, input(project, exc("ValueError")), ts(1))
            .await
            .unwrap();
        let b = ingest_normalized(&issues, &events, input(project, exc("KeyError")), ts(2))
            .await
            .unwrap();

        assert_ne!(a.issue.id, b.issue.id);
        assert_eq!(b.notify, NotifyKind::NewIssue);
        assert_eq!(issues.rows.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn resolved_issue_regresses_on_new_event() {
        let issues = FakeIssues::default();
        let events = FakeEvents::default();
        let project = Id::new_v4();

        let first = ingest_normalized(&issues, &events, input(project, exc("ValueError")), ts(100))
            .await
            .unwrap();

        // Operator resolves the issue.
        issues
            .set_status(first.issue.id, IssueStatus::Resolved)
            .await
            .unwrap();

        // A new matching event arrives → regression.
        let regressed =
            ingest_normalized(&issues, &events, input(project, exc("ValueError")), ts(300))
                .await
                .unwrap();

        assert_eq!(regressed.issue.id, first.issue.id);
        assert_eq!(regressed.notify, NotifyKind::Regression);
        assert_eq!(regressed.issue.status, IssueStatus::Unresolved);
        assert_eq!(regressed.issue.event_count, 2);
    }

    #[tokio::test]
    async fn muted_issue_still_counts_but_is_caller_concern() {
        // The pipeline counts events regardless of mute (§8.1 — mute never drops
        // ingestion). Mute suppression is applied by the caller, so a recurring
        // event on a muted issue yields NotifyKind::None here anyway.
        let issues = FakeIssues::default();
        let events = FakeEvents::default();
        let project = Id::new_v4();

        let first = ingest_normalized(&issues, &events, input(project, exc("ValueError")), ts(1))
            .await
            .unwrap();
        issues
            .set_status(first.issue.id, IssueStatus::Muted)
            .await
            .unwrap();

        let second = ingest_normalized(&issues, &events, input(project, exc("ValueError")), ts(2))
            .await
            .unwrap();

        assert_eq!(second.issue.event_count, 2, "muting never drops counting");
        assert_eq!(second.notify, NotifyKind::None);
    }
}

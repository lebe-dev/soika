//! In-process background scheduler — retention cleanup.
//!
//! Runs as a `tokio` task in the same runtime as the HTTP server (no broker,
//! ). Parses `RETENTION_CRON` (croner) and enforces per-project event
//! retention on schedule. Two complementary policies run per project:
//!
//!   * count-based: prune events over each project's `retention_events`
//!     (default `DEFAULT_EVENTS_RETENTION`);
//!   * age-based: prune events older than `retention_days`
//!     (default `DEFAULT_RETENTION_DAYS`; 0 disables).
//!
//! In both cases the issue-level aggregate counters are preserved by the
//! repository — only `events` rows are deleted.
//!
//! Outbound mail is sent inline from the ingest pipeline via [`crate::notify`];
//! the scheduler owns only periodic, time-driven work.

use std::time::Duration;

use croner::Cron;

use crate::domain::Timestamp;
use crate::error::{Error, Result};
use crate::state::AppState;

/// Largest single sleep between schedule checks. Capping the wait keeps the loop
/// responsive to abort/shutdown and avoids overflow when the next occurrence is
/// far away.
const MAX_SLEEP: Duration = Duration::from_secs(60);

/// Spawn the background scheduler as a detached `tokio` task.
///
/// The returned handle is aborted on shutdown (see `main`). If `RETENTION_CRON`
/// fails to parse, the task logs the error and exits without scheduling — the
/// HTTP server keeps running (retention is best-effort, not load-bearing).
pub fn spawn(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(err) = run_loop(&state).await {
            tracing::error!(error = %err, "scheduler: stopped");
        }
    })
}

/// The scheduler loop: parse the cron, then repeatedly sleep until the next
/// occurrence and run the retention sweep.
async fn run_loop(state: &AppState) -> Result<()> {
    let cron = parse_cron(&state.config.retention_cron)?;
    tracing::info!(cron = %state.config.retention_cron, "scheduler: started");

    // The next scheduled fire time, recomputed after each run from the previous
    // target (exclusive) so we never fire the same occurrence twice.
    let mut target = next_occurrence(&cron, state.clock.now())?;

    loop {
        let now = state.clock.now();
        if now < target {
            // Cap each sleep so abort()/shutdown takes effect promptly even when
            // the next occurrence is far away (sparse crons).
            let wait = sleep_until(target, now);
            tokio::time::sleep(wait).await;
            continue;
        }

        if let Err(err) = run_retention(state).await {
            // A failed sweep must not kill the scheduler; log and continue.
            tracing::warn!(error = %err, "scheduler: retention sweep failed");
        }

        // Schedule the following occurrence relative to the one we just fired, so
        // a slow sweep can't cause us to skip an interval back-to-back.
        target = next_occurrence(&cron, target)?;
    }
}

/// Parse a cron expression, accepting both 5-field and 6-field (seconds) forms.
///
/// The MVP default `0 */15 * * * *` is a 6-field pattern; `with_seconds_optional`
/// lets operators also supply a standard 5-field crontab line.
pub fn parse_cron(expr: &str) -> Result<Cron> {
    Cron::new(expr)
        .with_seconds_optional()
        .parse()
        .map_err(|e| Error::validation(format!("invalid RETENTION_CRON {expr:?}: {e}")))
}

/// The first occurrence strictly after `after`.
fn next_occurrence(cron: &Cron, after: Timestamp) -> Result<Timestamp> {
    cron.find_next_occurrence(&after, false)
        .map_err(|e| Error::internal(format!("computing next cron occurrence: {e}")))
}

/// How long to sleep now, capped at [`MAX_SLEEP`] so the loop stays responsive.
///
/// Returns [`Duration::ZERO`] when `target` is already at/behind `now`.
fn sleep_until(target: Timestamp, now: Timestamp) -> Duration {
    let remaining = (target - now).to_std().unwrap_or(Duration::ZERO);
    remaining.min(MAX_SLEEP)
}

/// Run a single retention sweep across all projects with events.
///
/// For each project, applies two complementary policies, both falling back to
/// the configured default when the project's own value is non-positive:
///
/// 1. count-based: prune events beyond `retention_events`;
/// 2. age-based: when the effective `retention_days` is > 0, delete
///    events older than `now - days`.
///
/// Issue aggregate counters are preserved by the repository
/// ([`prune_events_over_retention`](crate::ports::EventRepository::prune_events_over_retention),
/// [`delete_older_than`](crate::ports::EventRepository::delete_older_than)); only
/// `events` rows are deleted.
pub async fn run_retention(state: &AppState) -> Result<()> {
    let default_retention = state.config.default_events_retention;
    let default_days = state.config.default_retention_days;
    let project_ids = state.events.project_ids_with_events().await?;

    let mut total_deleted: u64 = 0;
    for project_id in project_ids {
        // Project deleted but events remain (FK race / orphan): treat as zeros so
        // the configured defaults still bound disk usage.
        let (project_retention, project_days) = match state.projects.find_by_id(project_id).await? {
            Some(project) => (project.retention_events, project.retention_days),
            None => (0, 0),
        };

        // 1. Count-based prune.
        let retention = effective_retention(project_retention, default_retention);
        let deleted = state
            .events
            .prune_events_over_retention(project_id, retention)
            .await?;
        if deleted > 0 {
            tracing::debug!(%project_id, deleted, retention, "scheduler: pruned events (count)");
        }
        total_deleted += deleted;

        // 2. Age-based prune. 0 = disabled.
        let days = effective_retention_days(project_days, default_days);
        if days > 0 {
            let cutoff = state.clock.now() - chrono::Duration::days(days);
            let deleted_by_age = state.events.delete_older_than(project_id, cutoff).await?;
            if deleted_by_age > 0 {
                tracing::debug!(%project_id, deleted = deleted_by_age, days, "scheduler: pruned events (age)");
            }
            total_deleted += deleted_by_age;
        }
    }

    if total_deleted > 0 {
        tracing::info!(total_deleted, "scheduler: retention sweep complete");
    }
    Ok(())
}

/// Resolve the retention limit for a project, defaulting when its own value is
/// unset/non-positive.
fn effective_retention(project_retention: i64, default_retention: i64) -> i64 {
    if project_retention > 0 {
        return project_retention;
    }
    default_retention.max(0)
}

/// Resolve the age-based retention window (days) for a project, defaulting when
/// its own value is unset/non-positive. 0 means disabled.
fn effective_retention_days(project_days: i64, default_days: i64) -> i64 {
    if project_days > 0 {
        return project_days;
    }
    default_days.max(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn parses_default_six_field_cron() {
        // The MVP default `RETENTION_CRON`.
        assert!(parse_cron("0 */15 * * * *").is_ok());
    }

    #[test]
    fn parses_five_field_cron() {
        // Standard crontab line (no seconds) is also accepted.
        assert!(parse_cron("*/15 * * * *").is_ok());
    }

    #[test]
    fn rejects_garbage_cron() {
        let err = parse_cron("not a cron").unwrap_err();
        assert!(matches!(err, Error::Validation(_)));
    }

    #[test]
    fn rejects_empty_cron() {
        assert!(matches!(parse_cron("   "), Err(Error::Validation(_))));
    }

    #[test]
    fn next_occurrence_is_the_following_boundary() {
        // At 12:00:00, the next `*/15` minute boundary is 12:15:00.
        let cron = parse_cron("0 */15 * * * *").unwrap();
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        let next = next_occurrence(&cron, now).unwrap();
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 1, 1, 12, 15, 0).unwrap());
    }

    #[test]
    fn next_occurrence_skips_the_current_instant() {
        // Exactly on a boundary, `inclusive = false` means the *next* one.
        let cron = parse_cron("0 */15 * * * *").unwrap();
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 15, 0).unwrap();
        let next = next_occurrence(&cron, now).unwrap();
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 1, 1, 12, 30, 0).unwrap());
    }

    #[test]
    fn sleep_until_caps_long_waits() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        // Target an hour out; the sleep must be capped at MAX_SLEEP.
        let target = Utc.with_ymd_and_hms(2026, 1, 1, 13, 0, 0).unwrap();
        assert_eq!(sleep_until(target, now), MAX_SLEEP);
    }

    #[test]
    fn sleep_until_returns_remaining_when_short() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        let target = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 30).unwrap();
        assert_eq!(sleep_until(target, now), Duration::from_secs(30));
    }

    #[test]
    fn sleep_until_is_zero_when_target_passed() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 30).unwrap();
        let target = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        assert_eq!(sleep_until(target, now), Duration::ZERO);
    }

    #[test]
    fn effective_retention_prefers_project_value() {
        assert_eq!(effective_retention(250, 1000), 250);
    }

    #[test]
    fn effective_retention_falls_back_on_nonpositive() {
        assert_eq!(effective_retention(0, 1000), 1000);
        assert_eq!(effective_retention(-5, 1000), 1000);
    }

    #[test]
    fn effective_retention_never_negative() {
        assert_eq!(effective_retention(0, -1), 0);
    }

    #[test]
    fn effective_retention_days_prefers_project_value() {
        assert_eq!(effective_retention_days(7, 30), 7);
    }

    #[test]
    fn effective_retention_days_falls_back_on_nonpositive() {
        assert_eq!(effective_retention_days(0, 30), 30);
        assert_eq!(effective_retention_days(-5, 30), 30);
    }

    #[test]
    fn effective_retention_days_never_negative() {
        assert_eq!(effective_retention_days(0, -1), 0);
    }

    // --- Integration: the retention sweep against real SQLite adapters --------

    use crate::config::Config;
    use crate::domain::Id;
    use crate::ports::{IssueUpsert, NewEvent, NewProject};

    fn test_config(default_events_retention: i64) -> Config {
        test_config_with(default_events_retention, 0)
    }

    /// Like [`test_config`] but also sets the age-based retention default
    /// (`DEFAULT_RETENTION_DAYS`).
    fn test_config_with(default_events_retention: i64, default_retention_days: i64) -> Config {
        Config {
            organization_name: "soika".into(),
            database_url: "sqlite::memory:".into(),
            bind_addr: "127.0.0.1:0".into(),
            base_url: "http://localhost:8080".into(),
            secret_key: "secret".into(),
            allow_signup: false,
            default_events_retention,
            default_retention_days,
            retention_cron: "0 */15 * * * *".into(),
            smtp: None,
            oidc: None,
        }
    }

    /// Ingest `n` events for `project_id` (one issue, increasing timestamps),
    /// mirroring real ingestion: each upsert bumps the issue counter and a row
    /// is written to `events`. Returns the issue id.
    async fn ingest_n(state: &AppState, project_id: Id, n: i64, base: Timestamp) -> Id {
        let mut issue_id = None;
        for i in 0..n {
            let at = base + chrono::Duration::seconds(i);
            let outcome = state
                .issues
                .upsert_by_fingerprint(IssueUpsert {
                    project_id,
                    fingerprint: "fp".into(),
                    title: "Boom".into(),
                    culprit: None,
                    level: Some("error".into()),
                    environment: None,
                    release: None,
                    seen_at: at,
                })
                .await
                .unwrap();
            let id = outcome.issue.id;
            issue_id = Some(id);
            state
                .events
                .insert(NewEvent {
                    event_id: format!("event-{i}"),
                    issue_id: id,
                    project_id,
                    payload: serde_json::json!({ "message": "boom" }),
                    received_at: at,
                })
                .await
                .unwrap();
        }
        issue_id.expect("at least one event")
    }

    #[tokio::test]
    async fn run_retention_prunes_per_project_value_and_default_fallback() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        crate::MIGRATOR.run(&pool).await.unwrap();
        // default_events_retention = 3 drives the project whose own value is 0.
        let state = crate::build_state(pool, test_config(3));

        let team = state.teams.create("team".into()).await.unwrap();
        // project_a keeps its own retention (2); project_b is non-positive and
        // must fall back to the configured default (3).
        let project_a = state
            .projects
            .create(NewProject {
                team_id: team.id,
                name: "A".into(),
                slug: "a".into(),
                dsn_public_key: "dsn-a".into(),
                retention_events: 2,
                retention_days: 0,
                webhook_url: None,
            })
            .await
            .unwrap();
        let project_b = state
            .projects
            .create(NewProject {
                team_id: team.id,
                name: "B".into(),
                slug: "b".into(),
                dsn_public_key: "dsn-b".into(),
                retention_events: 0,
                retention_days: 0,
                webhook_url: None,
            })
            .await
            .unwrap();

        let base = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let issue_a = ingest_n(&state, project_a.id, 5, base).await;
        ingest_n(&state, project_b.id, 4, base).await;

        run_retention(&state).await.unwrap();

        assert_eq!(
            state.events.count_for_project(project_a.id).await.unwrap(),
            2,
            "project keeps its own retention value"
        );
        assert_eq!(
            state.events.count_for_project(project_b.id).await.unwrap(),
            3,
            "non-positive project retention falls back to the default"
        );

        // Aggregate counters live on the issue and survive event pruning.
        let issue = state.issues.find_by_id(issue_a).await.unwrap().unwrap();
        assert_eq!(
            issue.event_count, 5,
            "pruning events must not rewind the issue counter"
        );
    }

    #[tokio::test]
    async fn run_retention_prunes_by_age_and_preserves_counters() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        crate::MIGRATOR.run(&pool).await.unwrap();
        // High count retention so count-based pruning never fires here; the
        // config default drives only the project whose own retention_days is 0.
        let state = crate::build_state(pool, test_config_with(10_000, 14));

        let team = state.teams.create("team".into()).await.unwrap();
        // project_a uses its OWN age retention (7 days). project_b leaves it at 0
        // and must fall back to the config default (14 days).
        let project_a = state
            .projects
            .create(NewProject {
                team_id: team.id,
                name: "A".into(),
                slug: "a".into(),
                dsn_public_key: "dsn-a".into(),
                retention_events: 10_000,
                retention_days: 7,
                webhook_url: None,
            })
            .await
            .unwrap();
        let project_b = state
            .projects
            .create(NewProject {
                team_id: team.id,
                name: "B".into(),
                slug: "b".into(),
                dsn_public_key: "dsn-b".into(),
                retention_events: 10_000,
                retention_days: 0,
                webhook_url: None,
            })
            .await
            .unwrap();

        // The scheduler computes the cutoff from `state.clock.now()` (wall clock),
        // so anchor the test timestamps RELATIVE to it. All events are ~30 days
        // old, comfortably past both the 7-day and 14-day windows, so every event
        // is pruned and there is no boundary ambiguity.
        let now = state.clock.now();
        let base = now - chrono::Duration::days(30);
        let issue_a = ingest_n(&state, project_a.id, 5, base).await;
        ingest_n(&state, project_b.id, 4, base).await;

        run_retention(&state).await.unwrap();

        assert_eq!(
            state.events.count_for_project(project_a.id).await.unwrap(),
            0,
            "events older than the project's own age window are pruned"
        );
        assert_eq!(
            state.events.count_for_project(project_b.id).await.unwrap(),
            0,
            "events older than the config default age window are pruned"
        );

        // Aggregate counters live on the issue and survive age-based pruning.
        let issue = state.issues.find_by_id(issue_a).await.unwrap().unwrap();
        assert_eq!(
            issue.event_count, 5,
            "age-based pruning must not rewind the issue counter"
        );
    }

    #[tokio::test]
    async fn run_retention_age_disabled_keeps_recent_events() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        crate::MIGRATOR.run(&pool).await.unwrap();
        // Count retention high and BOTH age defaults disabled (0): nothing pruned.
        let state = crate::build_state(pool, test_config_with(10_000, 0));

        let team = state.teams.create("team".into()).await.unwrap();
        let project = state
            .projects
            .create(NewProject {
                team_id: team.id,
                name: "C".into(),
                slug: "c".into(),
                dsn_public_key: "dsn-c".into(),
                retention_events: 10_000,
                retention_days: 0,
                webhook_url: None,
            })
            .await
            .unwrap();

        let base = state.clock.now() - chrono::Duration::days(30);
        ingest_n(&state, project.id, 3, base).await;

        run_retention(&state).await.unwrap();

        assert_eq!(
            state.events.count_for_project(project.id).await.unwrap(),
            3,
            "age-based pruning disabled (0) keeps events regardless of age"
        );
    }
}

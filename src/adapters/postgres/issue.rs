//! PostgreSQL [`IssueRepository`] adapter.
//!
//! Grouping & regression logic lives in [`upsert_by_fingerprint`]: one issue per
//! `(project_id, fingerprint)`; a new event on a `resolved` issue flips it back
//! to `unresolved` (regression).
//!
//! Postgres runs concurrent writers, so the read-then-write paths are made safe
//! with the unique `(project_id, fingerprint)` index and row locks rather than
//! SQLite's database-wide write lock:
//!
//!   * upsert — the existing row is locked (`FOR UPDATE`) and bumped; an unseen
//!     fingerprint is inserted with `INSERT … ON CONFLICT DO NOTHING`, so a race
//!     with a concurrent insert is settled atomically.
//!   * fingerprint override — target and collision rows are locked; a rename
//!     that loses a race to a concurrent insert is re-run as a merge.

use super::{Db, is_unique_violation, now, ts, unique_short_id_on};
use crate::adapters::retry::retry_write;
use crate::config::WritePolicy;
use crate::domain::{Id, Issue, IssueStatus, MuteSpec, Timestamp};
use crate::error::{Error, Result};
use crate::ports::{IssueFilter, IssueRepository, IssueSort, IssueUpsert, UpsertOutcome};
use async_trait::async_trait;
use sqlx::{AssertSqlSafe, FromRow, Postgres, Transaction};
use std::collections::HashMap;
use std::str::FromStr;

/// How many times a read-then-write that lost a race to a concurrent writer is
/// re-run before giving up. Each re-run sees the winner's committed row, so a
/// second attempt practically always settles it.
const RACE_ATTEMPTS: usize = 3;

#[derive(Clone)]
pub struct PgIssueRepository {
    db: Db,
    write: WritePolicy,
}

impl PgIssueRepository {
    pub fn new(db: Db) -> Self {
        PgIssueRepository {
            db,
            write: WritePolicy::default(),
        }
    }

    /// Override the write retry policy (wired from `DB_WRITE_*` in
    /// `soika::build_state`); [`new`](Self::new) uses the defaults.
    pub fn with_write_policy(mut self, write: WritePolicy) -> Self {
        self.write = write;
        self
    }

    /// One transactional attempt at [`IssueRepository::upsert_by_fingerprint`].
    ///
    /// The common case — a repeat event — is one locking read plus one update.
    /// Only an unseen fingerprint allocates a short id and inserts. Returns
    /// `None` when the issue vanished between a conflicting insert and the row
    /// lock (deleted concurrently); the caller simply runs it again.
    async fn upsert_once(&self, upsert: &IssueUpsert) -> Result<Option<UpsertOutcome>> {
        let seen = ts(upsert.seen_at);
        let mut tx = self.db.begin().await?;

        if let Some(current) = lock_by_fingerprint(&mut tx, upsert).await? {
            let outcome = bump_existing(&mut tx, current, upsert, seen).await?;
            tx.commit().await?;
            return Ok(Some(outcome));
        }

        // New fingerprint → brand-new unresolved issue (new-issue notice).
        // environment/release (like culprit/level) are set on first sight only.
        // `ON CONFLICT DO NOTHING` settles a race with a concurrent insert of the
        // same fingerprint atomically.
        let short_id = unique_short_id_on(&mut tx, "issues").await?;
        let inserted = sqlx::query_as::<_, IssueRow>(AssertSqlSafe(format!(
            "INSERT INTO issues \
                (id, short_id, project_id, fingerprint, title, culprit, level, \
                 environment, release, status, \
                 first_seen, last_seen, event_count, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'unresolved', $10, $10, 1, $10, $10) \
             ON CONFLICT (project_id, fingerprint) DO NOTHING \
             RETURNING {ISSUE_COLS}"
        )))
        .bind(Id::new_v4())
        .bind(short_id)
        .bind(upsert.project_id)
        .bind(&upsert.fingerprint)
        .bind(&upsert.title)
        .bind(&upsert.culprit)
        .bind(&upsert.level)
        .bind(&upsert.environment)
        .bind(&upsert.release)
        .bind(seen)
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(row) = inserted {
            let issue = row.into_domain()?;
            tx.commit().await?;
            return Ok(Some(UpsertOutcome {
                issue,
                is_new: true,
                is_regression: false,
            }));
        }

        // Lost the race: a concurrent writer committed this fingerprint first.
        // Under READ COMMITTED the next statement sees that row, so bump it.
        let Some(current) = lock_by_fingerprint(&mut tx, upsert).await? else {
            return Ok(None);
        };
        let outcome = bump_existing(&mut tx, current, upsert, seen).await?;
        tx.commit().await?;
        Ok(Some(outcome))
    }

    /// One transactional attempt at [`IssueRepository::override_fingerprint`].
    ///
    /// Returns `None` when the rename lost a race: another writer inserted
    /// `new_fingerprint` after the collision check. Re-running it then takes the
    /// merge branch.
    async fn override_fingerprint_once(
        &self,
        issue_id: Id,
        new_fingerprint: &str,
    ) -> Result<Option<Issue>> {
        let mut tx = self.db.begin().await?;
        let now = now();

        // 1. Lock the target issue (the row identified by `issue_id` always
        //    survives — a merge deletes the *other* colliding row).
        let target = sqlx::query_as::<_, IssueRow>(AssertSqlSafe(format!(
            "SELECT {ISSUE_COLS} FROM issues WHERE id = $1 FOR UPDATE"
        )))
        .bind(issue_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(target) = target else {
            return Err(Error::not_found(format!("issue {issue_id}")));
        };
        let target = target.into_domain()?;

        // 2. No-op: the fingerprint is unchanged.
        if target.fingerprint == new_fingerprint {
            tx.commit().await?;
            return Ok(Some(target));
        }

        // 3. Look for (and lock) a colliding issue in the same project — the
        //    merge source.
        let collision = sqlx::query_as::<_, IssueRow>(AssertSqlSafe(format!(
            "SELECT {ISSUE_COLS} FROM issues \
             WHERE project_id = $1 AND fingerprint = $2 AND id <> $3 FOR UPDATE"
        )))
        .bind(target.project_id)
        .bind(new_fingerprint)
        .bind(issue_id)
        .fetch_optional(&mut *tx)
        .await?;

        let updated = if let Some(source) = collision {
            // --- MERGE branch ------------------------------------------------
            let source = source.into_domain()?;

            // Fold aggregates (earliest first_seen, latest last_seen).
            let first_seen = target.first_seen.min(source.first_seen);
            let last_seen = target.last_seen.max(source.last_seen);
            let event_count = target.event_count + source.event_count;

            // (a) Re-point the source's events onto the target FIRST, so the
            //     subsequent DELETE does not CASCADE-delete them.
            sqlx::query("UPDATE events SET issue_id = $1 WHERE issue_id = $2")
                .bind(target.id)
                .bind(source.id)
                .execute(&mut *tx)
                .await?;

            // (b) Delete the source BEFORE assigning its fingerprint to the
            //     target, otherwise the unique index would see two holders.
            sqlx::query("DELETE FROM issues WHERE id = $1")
                .bind(source.id)
                .execute(&mut *tx)
                .await?;

            // (c) Fold the aggregates onto the surviving target and adopt the
            //     new fingerprint.
            sqlx::query_as::<_, IssueRow>(AssertSqlSafe(format!(
                "UPDATE issues SET \
                    fingerprint = $1, \
                    event_count = $2, \
                    first_seen = $3, \
                    last_seen = $4, \
                    updated_at = $5 \
                 WHERE id = $6 RETURNING {ISSUE_COLS}"
            )))
            .bind(new_fingerprint)
            .bind(event_count)
            .bind(first_seen)
            .bind(last_seen)
            .bind(now)
            .bind(target.id)
            .fetch_one(&mut *tx)
            .await?
        } else {
            // --- RENAME branch (split) --------------------------------------
            let renamed = sqlx::query_as::<_, IssueRow>(AssertSqlSafe(format!(
                "UPDATE issues SET fingerprint = $1, updated_at = $2 \
                 WHERE id = $3 RETURNING {ISSUE_COLS}"
            )))
            .bind(new_fingerprint)
            .bind(now)
            .bind(target.id)
            .fetch_one(&mut *tx)
            .await;
            match renamed {
                Ok(row) => row,
                // Dropping `tx` rolls the attempt back.
                Err(e) if is_unique_violation(&e) => return Ok(None),
                Err(e) => return Err(e.into()),
            }
        };

        let issue = updated.into_domain()?;
        tx.commit().await?;
        Ok(Some(issue))
    }
}

const ISSUE_COLS: &str = "id, short_id, project_id, fingerprint, title, culprit, level, \
     environment, release, status, \
     muted_at, muted_until, mute_threshold, mute_window_seconds, \
     first_seen, last_seen, event_count, created_at, updated_at";

#[derive(FromRow)]
struct IssueRow {
    id: Id,
    short_id: String,
    project_id: Id,
    fingerprint: String,
    title: String,
    culprit: Option<String>,
    level: Option<String>,
    environment: Option<String>,
    release: Option<String>,
    status: String,
    muted_at: Option<Timestamp>,
    muted_until: Option<Timestamp>,
    mute_threshold: Option<i64>,
    mute_window_seconds: Option<i64>,
    first_seen: Timestamp,
    last_seen: Timestamp,
    event_count: i64,
    created_at: Timestamp,
    updated_at: Timestamp,
}

impl IssueRow {
    fn into_domain(self) -> Result<Issue> {
        Ok(Issue {
            id: self.id,
            short_id: self.short_id,
            project_id: self.project_id,
            fingerprint: self.fingerprint,
            title: self.title,
            culprit: self.culprit,
            level: self.level,
            environment: self.environment,
            release: self.release,
            status: IssueStatus::from_str(&self.status)?,
            muted_at: self.muted_at,
            muted_until: self.muted_until,
            mute_threshold: self.mute_threshold,
            mute_window_seconds: self.mute_window_seconds,
            first_seen: self.first_seen,
            last_seen: self.last_seen,
            event_count: self.event_count,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

/// Lock the issue of `upsert`'s `(project_id, fingerprint)`, if it exists, so
/// the regression check and the bump see the same state as concurrent ingestion
/// of this fingerprint.
async fn lock_by_fingerprint(
    tx: &mut Transaction<'_, Postgres>,
    upsert: &IssueUpsert,
) -> Result<Option<Issue>> {
    sqlx::query_as::<_, IssueRow>(AssertSqlSafe(format!(
        "SELECT {ISSUE_COLS} FROM issues \
         WHERE project_id = $1 AND fingerprint = $2 FOR UPDATE"
    )))
    .bind(upsert.project_id)
    .bind(&upsert.fingerprint)
    .fetch_optional(&mut **tx)
    .await?
    .map(IssueRow::into_domain)
    .transpose()
}

/// Record one more event on a locked, existing issue.
async fn bump_existing(
    tx: &mut Transaction<'_, Postgres>,
    current: Issue,
    upsert: &IssueUpsert,
    seen: Timestamp,
) -> Result<UpsertOutcome> {
    let is_regression = current.status == IssueStatus::Resolved;
    // A regression flips resolved → unresolved; muted stays muted so it keeps
    // suppressing notifications. last_seen always advances.
    let new_status = if is_regression {
        IssueStatus::Unresolved
    } else {
        current.status
    };
    // Refresh the title on every event (a stable, parameterized form, so this is
    // idempotent within a group and self-heals titles frozen from an early raw
    // sample). culprit/level/environment/release keep the first-observed values
    // by design.
    let updated = sqlx::query_as::<_, IssueRow>(AssertSqlSafe(format!(
        "UPDATE issues SET \
            status = $1, \
            title = $2, \
            last_seen = $3, \
            event_count = event_count + 1, \
            updated_at = $3 \
         WHERE id = $4 RETURNING {ISSUE_COLS}"
    )))
    .bind(new_status.as_str())
    .bind(&upsert.title)
    .bind(seen)
    .bind(current.id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(UpsertOutcome {
        issue: updated.into_domain()?,
        is_new: false,
        is_regression,
    })
}

impl PgIssueRepository {
    /// Fetch at most one issue matching `WHERE {predicate}` with `$1` bound.
    async fn find_one<T>(&self, predicate: &str, value: T) -> Result<Option<Issue>>
    where
        T: for<'q> sqlx::Encode<'q, sqlx::Postgres> + sqlx::Type<sqlx::Postgres> + Send,
    {
        let row = sqlx::query_as::<_, IssueRow>(AssertSqlSafe(format!(
            "SELECT {ISSUE_COLS} FROM issues WHERE {predicate}"
        )))
        .bind(value)
        .fetch_optional(&self.db)
        .await?;
        row.map(IssueRow::into_domain).transpose()
    }
}

#[async_trait]
impl IssueRepository for PgIssueRepository {
    async fn find_by_id(&self, id: Id) -> Result<Option<Issue>> {
        self.find_one("id = $1", id).await
    }

    async fn find_by_short_id(&self, short_id: &str) -> Result<Option<Issue>> {
        self.find_one("short_id = $1", short_id).await
    }

    async fn find_by_fingerprint(
        &self,
        project_id: Id,
        fingerprint: &str,
    ) -> Result<Option<Issue>> {
        let row = sqlx::query_as::<_, IssueRow>(AssertSqlSafe(format!(
            "SELECT {ISSUE_COLS} FROM issues WHERE project_id = $1 AND fingerprint = $2"
        )))
        .bind(project_id)
        .bind(fingerprint)
        .fetch_optional(&self.db)
        .await?;
        row.map(IssueRow::into_domain).transpose()
    }

    async fn upsert_by_fingerprint(&self, upsert: IssueUpsert) -> Result<UpsertOutcome> {
        // The hottest write in the system (one per ingested event): each attempt
        // is one transaction, retried as a whole on deadlock / serialization
        // failure, and re-run if it lost a race to a concurrent delete.
        for _ in 0..RACE_ATTEMPTS {
            let outcome = retry_write(&self.write, "upsert issue by fingerprint", || {
                self.upsert_once(&upsert)
            })
            .await?;
            if let Some(outcome) = outcome {
                return Ok(outcome);
            }
        }
        Err(Error::internal(
            "upsert issue by fingerprint kept losing races to concurrent deletes",
        ))
    }

    async fn set_status(&self, issue_id: Id, status: IssueStatus) -> Result<Issue> {
        // Any non-muted status clears the mute bookkeeping so a re-opened or
        // resolved issue never carries a stale expiry/threshold.
        let row = sqlx::query_as::<_, IssueRow>(AssertSqlSafe(format!(
            "UPDATE issues SET status = $1, \
                muted_at = NULL, muted_until = NULL, \
                mute_threshold = NULL, mute_window_seconds = NULL, \
                updated_at = $2 \
             WHERE id = $3 RETURNING {ISSUE_COLS}"
        )))
        .bind(status.as_str())
        .bind(now())
        .bind(issue_id)
        .fetch_optional(&self.db)
        .await?;
        row.ok_or_else(|| Error::not_found(format!("issue {issue_id}")))?
            .into_domain()
    }

    async fn mute(&self, issue_id: Id, spec: MuteSpec, now: Timestamp) -> Result<Issue> {
        // Translate the spec into the three mutually-exclusive column groups;
        // `Forever` leaves period/rate NULL (only `muted_at` is recorded).
        let (until, threshold, window) = match spec {
            MuteSpec::Forever => (None, None, None),
            MuteSpec::Until(until) => (Some(ts(until)), None, None),
            MuteSpec::EventRate {
                threshold,
                window_seconds,
            } => (None, Some(threshold), Some(window_seconds)),
        };
        let row = sqlx::query_as::<_, IssueRow>(AssertSqlSafe(format!(
            "UPDATE issues SET status = 'muted', \
                muted_at = $1, muted_until = $2, \
                mute_threshold = $3, mute_window_seconds = $4, \
                updated_at = $1 \
             WHERE id = $5 RETURNING {ISSUE_COLS}"
        )))
        .bind(ts(now))
        .bind(until)
        .bind(threshold)
        .bind(window)
        .bind(issue_id)
        .fetch_optional(&self.db)
        .await?;
        row.ok_or_else(|| Error::not_found(format!("issue {issue_id}")))?
            .into_domain()
    }

    async fn clear_expired_mutes(&self, now: Timestamp) -> Result<u64> {
        // Time-based mutes only (muted_until set & passed). Event-rate mutes
        // (muted_until NULL) are resurfaced by ingestion, never here.
        let result = sqlx::query(
            "UPDATE issues SET status = 'unresolved', \
                muted_at = NULL, muted_until = NULL, \
                mute_threshold = NULL, mute_window_seconds = NULL, \
                updated_at = $1 \
             WHERE status = 'muted' AND muted_until IS NOT NULL AND muted_until <= $1",
        )
        .bind(ts(now))
        .execute(&self.db)
        .await?;
        Ok(result.rows_affected())
    }

    async fn list(&self, project_id: Id, filter: IssueFilter) -> Result<Vec<Issue>> {
        // Each optional filter adds its predicate only when it is set. A static
        // `($2::text IS NULL OR status = $2)` form plans badly once Postgres
        // switches to a generic plan, so the `(project_id, status, last_seen)`
        // index could not serve the common "unresolved, newest first" page.
        // Predicates are fixed literals; every value is bound. `query` is a
        // case-insensitive substring match on title/culprit (ILIKE, matching
        // SQLite's ASCII-case-insensitive LIKE).
        let limit = filter.limit.unwrap_or(50).clamp(1, 500);
        let offset = filter.offset.unwrap_or(0).max(0);

        let mut sql = format!("SELECT {ISSUE_COLS} FROM issues WHERE project_id = $1");
        let mut binds: Vec<String> = Vec::new();
        let exact = [
            ("status", filter.status.map(|s| s.as_str().to_string())),
            ("level", filter.level),
            ("environment", filter.environment),
            ("release", filter.release),
        ];
        for (column, value) in exact {
            let Some(value) = value else { continue };
            binds.push(value);
            sql.push_str(&format!(" AND {column} = ${}", binds.len() + 1));
        }
        if let Some(query) = filter.query {
            binds.push(format!("%{query}%"));
            let n = binds.len() + 1;
            sql.push_str(&format!(" AND (title ILIKE ${n} OR culprit ILIKE ${n})"));
        }

        // ORDER BY is selected from fixed literals via an exhaustive enum match
        // (never from raw user input), so it stays injection-safe.
        let order_by = match filter.sort {
            IssueSort::LastSeen => "last_seen DESC",
            IssueSort::EventCount => "event_count DESC, last_seen DESC",
        };
        let n = binds.len() + 2;
        sql.push_str(&format!(
            " ORDER BY {order_by} LIMIT ${n} OFFSET ${}",
            n + 1
        ));

        let mut query = sqlx::query_as::<_, IssueRow>(AssertSqlSafe(sql)).bind(project_id);
        for value in binds {
            query = query.bind(value);
        }
        let rows = query.bind(limit).bind(offset).fetch_all(&self.db).await?;
        rows.into_iter().map(IssueRow::into_domain).collect()
    }

    async fn unresolved_counts(&self, project_ids: &[Id]) -> Result<HashMap<Id, i64>> {
        if project_ids.is_empty() {
            return Ok(HashMap::new());
        }
        // Projects with zero unresolved issues drop out of the GROUP BY and are
        // simply absent from the returned map.
        let rows: Vec<(Id, i64)> = sqlx::query_as(
            "SELECT project_id, COUNT(*) FROM issues \
             WHERE status = 'unresolved' AND project_id = ANY($1) \
             GROUP BY project_id",
        )
        .bind(project_ids)
        .fetch_all(&self.db)
        .await?;
        Ok(rows.into_iter().collect())
    }

    async fn last_seen_per_project(&self, project_ids: &[Id]) -> Result<HashMap<Id, Timestamp>> {
        if project_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows: Vec<(Id, Timestamp)> = sqlx::query_as(
            "SELECT project_id, MAX(last_seen) FROM issues \
             WHERE status = 'unresolved' AND project_id = ANY($1) \
             GROUP BY project_id",
        )
        .bind(project_ids)
        .fetch_all(&self.db)
        .await?;
        Ok(rows.into_iter().collect())
    }

    async fn override_fingerprint(&self, issue_id: Id, new_fingerprint: String) -> Result<Issue> {
        for _ in 0..RACE_ATTEMPTS {
            let issue = retry_write(&self.write, "override issue fingerprint", || {
                self.override_fingerprint_once(issue_id, &new_fingerprint)
            })
            .await?;
            if let Some(issue) = issue {
                return Ok(issue);
            }
        }
        Err(Error::Conflict(format!(
            "fingerprint {new_fingerprint} is being written concurrently; try again"
        )))
    }

    async fn delete(&self, id: Id) -> Result<()> {
        sqlx::query("DELETE FROM issues WHERE id = $1")
            .bind(id)
            .execute(&self.db)
            .await?;
        Ok(())
    }
}

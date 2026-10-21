//! SQLite [`IssueRepository`] adapter.
//!
//! Persistence notes (mirroring `migrations/0001_init.sql`):
//!   * `id` / `project_id` stored as TEXT (UUID v4 string form).
//!   * Timestamps TEXT in RFC3339/ISO-8601 (UTC).
//!   * `status` TEXT (`unresolved` | `resolved` | `muted`).
//!
//! Grouping & regression logic lives in
//! [`upsert_by_fingerprint`]: one issue per `(project_id, fingerprint)`; a new
//! event on a `resolved` issue flips it back to `unresolved` (regression).
//! SQL is ANSI-friendly; the only SQLite-specific bit is TEXT row decoding.

use super::{Db, begin_write, retry_write};
use crate::config::WritePolicy;
use crate::domain::{Id, Issue, IssueStatus, MuteSpec, Timestamp};
use crate::error::{Error, Result};
use crate::ports::{IssueFilter, IssueRepository, IssueSort, IssueUpsert, UpsertOutcome};
use async_trait::async_trait;
use sqlx::Row;
use std::collections::HashMap;
use std::str::FromStr;

#[derive(Clone)]
pub struct SqliteIssueRepository {
    db: Db,
    write: WritePolicy,
}

impl SqliteIssueRepository {
    pub fn new(db: Db) -> Self {
        SqliteIssueRepository {
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

    /// One attempt at [`IssueRepository::upsert_by_fingerprint`] — the whole
    /// read-then-write in a single `BEGIN IMMEDIATE` transaction, so
    /// [`retry_write`] can simply re-run it on a lock conflict.
    async fn upsert_once(&self, upsert: &IssueUpsert) -> Result<UpsertOutcome> {
        // Allocate the short id up front, before opening the transaction: the
        // uniqueness pre-check borrows a pooled connection, and the test pool is
        // pinned to a single connection — doing it while the tx holds that
        // connection would deadlock. Only the new-issue branch consumes it.
        let short_id = super::unique_short_id(&self.db, "issues").await?;

        // The read-then-write must be consistent under concurrent ingestion of
        // the same fingerprint, and it must announce itself as a writer: a
        // deferred `BEGIN` would start as a reader and fail outright (SQLITE_BUSY,
        // busy_timeout not applicable) when it later tried to upgrade.
        let mut tx = begin_write(&self.db).await?;
        let seen = upsert.seen_at.to_rfc3339();

        let existing_sql =
            format!("SELECT {ISSUE_COLS} FROM issues WHERE project_id = ? AND fingerprint = ?");
        let existing = sqlx::query(sqlx::AssertSqlSafe(&*existing_sql))
            .bind(upsert.project_id.to_string())
            .bind(&upsert.fingerprint)
            .fetch_optional(&mut *tx)
            .await?;

        if let Some(row) = existing {
            let current = row_to_issue(&row)?;
            let is_regression = current.status == IssueStatus::Resolved;
            // A regression flips resolved → unresolved; muted stays muted so it
            // keeps suppressing notifications. last_seen always advances.
            let new_status = if is_regression {
                IssueStatus::Unresolved
            } else {
                current.status
            };
            // Refresh the title on every event. It is a stable, parameterized
            // form (volatile tokens masked to <int>/<uuid>/<id>), so this is
            // idempotent within a group AND self-heals issues whose title was
            // frozen from an early raw sample. culprit/level/environment/release
            // stay untouched — those keep the first-observed values by design.
            let update_sql = format!(
                "UPDATE issues SET \
                    status = ?, \
                    title = ?, \
                    last_seen = ?, \
                    event_count = event_count + 1, \
                    updated_at = ? \
                 WHERE id = ? RETURNING {ISSUE_COLS}"
            );
            let updated_row = sqlx::query(sqlx::AssertSqlSafe(&*update_sql))
                .bind(new_status.as_str())
                .bind(&upsert.title)
                .bind(&seen)
                .bind(&seen)
                .bind(current.id.to_string())
                .fetch_one(&mut *tx)
                .await?;
            let issue = row_to_issue(&updated_row)?;
            tx.commit().await?;
            return Ok(UpsertOutcome {
                issue,
                is_new: false,
                is_regression,
            });
        }

        // New fingerprint → brand-new unresolved issue (new-issue notice).
        // environment/release are set here on first sight only; the UPDATE branch
        // above intentionally leaves them (and culprit/level) untouched so an
        // issue keeps the values from when it was first observed.
        let id = Id::new_v4();
        let insert_sql = format!(
            "INSERT INTO issues \
                (id, short_id, project_id, fingerprint, title, culprit, level, \
                 environment, release, status, \
                 first_seen, last_seen, event_count, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'unresolved', ?, ?, 1, ?, ?) \
             RETURNING {ISSUE_COLS}"
        );
        let inserted = sqlx::query(sqlx::AssertSqlSafe(&*insert_sql))
            .bind(id.to_string())
            .bind(short_id)
            .bind(upsert.project_id.to_string())
            .bind(&upsert.fingerprint)
            .bind(&upsert.title)
            .bind(&upsert.culprit)
            .bind(&upsert.level)
            .bind(&upsert.environment)
            .bind(&upsert.release)
            .bind(&seen)
            .bind(&seen)
            .bind(&seen)
            .bind(&seen)
            .fetch_one(&mut *tx)
            .await?;
        let issue = row_to_issue(&inserted)?;
        tx.commit().await?;
        Ok(UpsertOutcome {
            issue,
            is_new: true,
            is_regression: false,
        })
    }

    /// One attempt at [`IssueRepository::override_fingerprint`]: a single
    /// `BEGIN IMMEDIATE` transaction, re-runnable by [`retry_write`].
    async fn override_fingerprint_once(
        &self,
        issue_id: Id,
        new_fingerprint: &str,
    ) -> Result<Issue> {
        // One write transaction so the collision check + merge/rename is atomic
        // and respects the unique (project_id, fingerprint) index under
        // concurrency. `BEGIN IMMEDIATE`, not a deferred `BEGIN`: the read-then-
        // write would otherwise fail outright once another writer commits.
        let mut tx = begin_write(&self.db).await?;
        let now = chrono::Utc::now().to_rfc3339();

        // 1. Load the target issue (the row identified by `issue_id` always
        //    survives — a merge deletes the *other* colliding row).
        let target_sql = format!("SELECT {ISSUE_COLS} FROM issues WHERE id = ?");
        let target_row = sqlx::query(sqlx::AssertSqlSafe(&*target_sql))
            .bind(issue_id.to_string())
            .fetch_optional(&mut *tx)
            .await?;
        let target = match target_row {
            Some(row) => row_to_issue(&row)?,
            None => return Err(Error::not_found(format!("issue {issue_id}"))),
        };

        // 2. No-op: the fingerprint is unchanged. Avoid a pointless UPDATE and
        //    any same-row collision logic.
        if target.fingerprint == new_fingerprint {
            tx.commit().await?;
            return Ok(target);
        }

        // 3. Look for a colliding issue in the same project (the merge source).
        let collision_sql = format!(
            "SELECT {ISSUE_COLS} FROM issues \
             WHERE project_id = ? AND fingerprint = ? AND id <> ?"
        );
        let collision_row = sqlx::query(sqlx::AssertSqlSafe(&*collision_sql))
            .bind(target.project_id.to_string())
            .bind(new_fingerprint)
            .bind(issue_id.to_string())
            .fetch_optional(&mut *tx)
            .await?;

        let updated_row = if let Some(row) = collision_row {
            // --- MERGE branch ------------------------------------------------
            let source = row_to_issue(&row)?;

            // Fold aggregates in Rust (earliest first_seen, latest last_seen).
            let first_seen = target.first_seen.min(source.first_seen);
            let last_seen = target.last_seen.max(source.last_seen);
            let event_count = target.event_count + source.event_count;

            // (a) Re-point the source's events onto the target FIRST, so the
            //     subsequent DELETE does not CASCADE-delete them.
            sqlx::query("UPDATE events SET issue_id = ? WHERE issue_id = ?")
                .bind(target.id.to_string())
                .bind(source.id.to_string())
                .execute(&mut *tx)
                .await?;

            // (b) Delete the source BEFORE assigning its fingerprint to the
            //     target, otherwise the unique index would transiently see two
            //     rows holding `new_fingerprint`.
            sqlx::query("DELETE FROM issues WHERE id = ?")
                .bind(source.id.to_string())
                .execute(&mut *tx)
                .await?;

            // (c) Fold the aggregates onto the surviving target and adopt the
            //     new fingerprint.
            let update_sql = format!(
                "UPDATE issues SET \
                    fingerprint = ?, \
                    event_count = ?, \
                    first_seen = ?, \
                    last_seen = ?, \
                    updated_at = ? \
                 WHERE id = ? RETURNING {ISSUE_COLS}"
            );
            sqlx::query(sqlx::AssertSqlSafe(&*update_sql))
                .bind(new_fingerprint)
                .bind(event_count)
                .bind(first_seen.to_rfc3339())
                .bind(last_seen.to_rfc3339())
                .bind(&now)
                .bind(target.id.to_string())
                .fetch_one(&mut *tx)
                .await?
        } else {
            // --- RENAME branch (split) --------------------------------------
            let update_sql = format!(
                "UPDATE issues SET fingerprint = ?, updated_at = ? \
                 WHERE id = ? RETURNING {ISSUE_COLS}"
            );
            sqlx::query(sqlx::AssertSqlSafe(&*update_sql))
                .bind(new_fingerprint)
                .bind(&now)
                .bind(target.id.to_string())
                .fetch_one(&mut *tx)
                .await?
        };

        let issue = row_to_issue(&updated_row)?;
        tx.commit().await?;
        Ok(issue)
    }
}

const ISSUE_COLS: &str = "id, short_id, project_id, fingerprint, title, culprit, level, \
     environment, release, status, \
     muted_at, muted_until, mute_threshold, mute_window_seconds, \
     first_seen, last_seen, event_count, created_at, updated_at";

fn row_to_issue(row: &sqlx::sqlite::SqliteRow) -> Result<Issue> {
    let status: String = row.try_get("status")?;
    Ok(Issue {
        id: parse_id(row.try_get::<String, _>("id")?)?,
        short_id: row.try_get("short_id")?,
        project_id: parse_id(row.try_get::<String, _>("project_id")?)?,
        fingerprint: row.try_get("fingerprint")?,
        title: row.try_get("title")?,
        culprit: row.try_get("culprit")?,
        level: row.try_get("level")?,
        environment: row.try_get("environment")?,
        release: row.try_get("release")?,
        status: IssueStatus::from_str(&status)?,
        muted_at: parse_ts_opt(row.try_get::<Option<String>, _>("muted_at")?)?,
        muted_until: parse_ts_opt(row.try_get::<Option<String>, _>("muted_until")?)?,
        mute_threshold: row.try_get("mute_threshold")?,
        mute_window_seconds: row.try_get("mute_window_seconds")?,
        first_seen: parse_ts(row.try_get::<String, _>("first_seen")?)?,
        last_seen: parse_ts(row.try_get::<String, _>("last_seen")?)?,
        event_count: row.try_get("event_count")?,
        created_at: parse_ts(row.try_get::<String, _>("created_at")?)?,
        updated_at: parse_ts(row.try_get::<String, _>("updated_at")?)?,
    })
}

fn parse_id(s: String) -> Result<Id> {
    Id::parse_str(&s).map_err(|e| Error::internal(format!("invalid uuid in db: {e}")))
}

fn parse_ts(s: String) -> Result<Timestamp> {
    chrono::DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .map_err(|e| Error::internal(format!("invalid timestamp in db: {e}")))
}

/// Decode a nullable timestamp column (`None` → `None`).
fn parse_ts_opt(s: Option<String>) -> Result<Option<Timestamp>> {
    s.map(parse_ts).transpose()
}

#[async_trait]
impl IssueRepository for SqliteIssueRepository {
    async fn find_by_id(&self, id: Id) -> Result<Option<Issue>> {
        let sql = format!("SELECT {ISSUE_COLS} FROM issues WHERE id = ?");
        let row = sqlx::query(sqlx::AssertSqlSafe(&*sql))
            .bind(id.to_string())
            .fetch_optional(&self.db)
            .await?;
        row.as_ref().map(row_to_issue).transpose()
    }

    async fn find_by_short_id(&self, short_id: &str) -> Result<Option<Issue>> {
        let sql = format!("SELECT {ISSUE_COLS} FROM issues WHERE short_id = ?");
        let row = sqlx::query(sqlx::AssertSqlSafe(&*sql))
            .bind(short_id)
            .fetch_optional(&self.db)
            .await?;
        row.as_ref().map(row_to_issue).transpose()
    }

    async fn find_by_fingerprint(
        &self,
        project_id: Id,
        fingerprint: &str,
    ) -> Result<Option<Issue>> {
        let sql =
            format!("SELECT {ISSUE_COLS} FROM issues WHERE project_id = ? AND fingerprint = ?");
        let row = sqlx::query(sqlx::AssertSqlSafe(&*sql))
            .bind(project_id.to_string())
            .bind(fingerprint)
            .fetch_optional(&self.db)
            .await?;
        row.as_ref().map(row_to_issue).transpose()
    }

    async fn upsert_by_fingerprint(&self, upsert: IssueUpsert) -> Result<UpsertOutcome> {
        // This is the hottest write in the system (one per ingested event) and
        // the one that contends with every other writer, so it is retried as a
        // whole: `upsert_once` is a single transaction, hence safe to re-run.
        retry_write(&self.write, "upsert issue by fingerprint", || {
            self.upsert_once(&upsert)
        })
        .await
    }

    async fn set_status(&self, issue_id: Id, status: IssueStatus) -> Result<Issue> {
        // Any non-muted status clears the mute bookkeeping so a re-opened or
        // resolved issue never carries a stale expiry/threshold. (This method
        // is only used for resolve/unresolve; muting goes through `mute`.)
        let sql = format!(
            "UPDATE issues SET status = ?, \
                muted_at = NULL, muted_until = NULL, \
                mute_threshold = NULL, mute_window_seconds = NULL, \
                updated_at = ? \
             WHERE id = ? RETURNING {ISSUE_COLS}"
        );
        let row = sqlx::query(sqlx::AssertSqlSafe(&*sql))
            .bind(status.as_str())
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(issue_id.to_string())
            .fetch_optional(&self.db)
            .await?;
        match row {
            Some(r) => row_to_issue(&r),
            None => Err(Error::not_found(format!("issue {issue_id}"))),
        }
    }

    async fn mute(&self, issue_id: Id, spec: MuteSpec, now: Timestamp) -> Result<Issue> {
        // Translate the spec into the three mutually-exclusive column groups;
        // `Forever` leaves period/rate NULL (only `muted_at` is recorded).
        let (until, threshold, window) = match spec {
            MuteSpec::Forever => (None, None, None),
            MuteSpec::Until(until) => (Some(until.to_rfc3339()), None, None),
            MuteSpec::EventRate {
                threshold,
                window_seconds,
            } => (None, Some(threshold), Some(window_seconds)),
        };
        let now = now.to_rfc3339();
        let sql = format!(
            "UPDATE issues SET status = 'muted', \
                muted_at = ?, muted_until = ?, \
                mute_threshold = ?, mute_window_seconds = ?, \
                updated_at = ? \
             WHERE id = ? RETURNING {ISSUE_COLS}"
        );
        let row = sqlx::query(sqlx::AssertSqlSafe(&*sql))
            .bind(&now)
            .bind(&until)
            .bind(threshold)
            .bind(window)
            .bind(&now)
            .bind(issue_id.to_string())
            .fetch_optional(&self.db)
            .await?;
        match row {
            Some(r) => row_to_issue(&r),
            None => Err(Error::not_found(format!("issue {issue_id}"))),
        }
    }

    async fn clear_expired_mutes(&self, now: Timestamp) -> Result<u64> {
        // Time-based mutes only (muted_until set & passed). Event-rate mutes
        // (muted_until NULL) are resurfaced by ingestion, never here.
        let result = sqlx::query(
            "UPDATE issues SET status = 'unresolved', \
                muted_at = NULL, muted_until = NULL, \
                mute_threshold = NULL, mute_window_seconds = NULL, \
                updated_at = ? \
             WHERE status = 'muted' AND muted_until IS NOT NULL AND muted_until <= ?",
        )
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.db)
        .await?;
        Ok(result.rows_affected())
    }

    async fn list(&self, project_id: Id, filter: IssueFilter) -> Result<Vec<Issue>> {
        // Optional status + first-class field + text-search filters. We bind a
        // sentinel (NULL → match all) for each optional piece so the SQL text
        // stays static and ANSI-friendly (mirrors the status pattern):
        //   * status / level / environment / release: NULL → match all;
        //     otherwise exact-match equality.
        //   * query: NULL → match all; otherwise LIKE on title/culprit.
        let like = filter.query.as_ref().map(|q| format!("%{q}%"));
        let level = filter.level.clone();
        let environment = filter.environment.clone();
        let release = filter.release.clone();
        let limit = filter.limit.unwrap_or(50).clamp(1, 500);
        let offset = filter.offset.unwrap_or(0).max(0);

        // ORDER BY is selected from fixed literals via an exhaustive enum match
        // (never from raw user input), so it stays injection-safe and
        // ANSI-friendly.
        let order_by = match filter.sort {
            IssueSort::LastSeen => "last_seen DESC",
            IssueSort::EventCount => "event_count DESC, last_seen DESC",
        };

        let sql = format!(
            "SELECT {ISSUE_COLS} FROM issues \
             WHERE project_id = ? \
               AND (? IS NULL OR status = ?) \
               AND (? IS NULL OR level = ?) \
               AND (? IS NULL OR environment = ?) \
               AND (? IS NULL OR release = ?) \
               AND (? IS NULL OR title LIKE ? OR culprit LIKE ?) \
             ORDER BY {order_by} \
             LIMIT ? OFFSET ?"
        );
        let status_str = filter.status.map(|s| s.as_str().to_string());
        let rows = sqlx::query(sqlx::AssertSqlSafe(&*sql))
            .bind(project_id.to_string())
            .bind(&status_str)
            .bind(&status_str)
            .bind(&level)
            .bind(&level)
            .bind(&environment)
            .bind(&environment)
            .bind(&release)
            .bind(&release)
            .bind(&like)
            .bind(&like)
            .bind(&like)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.db)
            .await?;
        rows.iter().map(row_to_issue).collect()
    }

    async fn unresolved_counts(&self, project_ids: &[Id]) -> Result<HashMap<Id, i64>> {
        if project_ids.is_empty() {
            return Ok(HashMap::new());
        }

        // Bind every id as a parameter (the count is bounded by the caller's
        // project list, never user-supplied text) so the IN-list stays
        // injection-safe. Projects with zero unresolved issues drop out of the
        // GROUP BY and are simply absent from the returned map.
        let placeholders = vec!["?"; project_ids.len()].join(",");
        let sql = format!(
            "SELECT project_id, COUNT(*) AS cnt FROM issues \
             WHERE status = 'unresolved' AND project_id IN ({placeholders}) \
             GROUP BY project_id"
        );
        let mut query = sqlx::query(sqlx::AssertSqlSafe(&*sql));
        for id in project_ids {
            query = query.bind(id.to_string());
        }
        let rows = query.fetch_all(&self.db).await?;

        let mut counts = HashMap::with_capacity(rows.len());
        for row in &rows {
            let project_id = parse_id(row.try_get::<String, _>("project_id")?)?;
            counts.insert(project_id, row.try_get::<i64, _>("cnt")?);
        }
        Ok(counts)
    }

    async fn last_seen_per_project(&self, project_ids: &[Id]) -> Result<HashMap<Id, Timestamp>> {
        if project_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders = vec!["?"; project_ids.len()].join(",");
        let sql = format!(
            "SELECT project_id, MAX(last_seen) AS last_seen FROM issues \
             WHERE status = 'unresolved' AND project_id IN ({placeholders}) \
             GROUP BY project_id"
        );
        let mut query = sqlx::query(sqlx::AssertSqlSafe(&*sql));
        for id in project_ids {
            query = query.bind(id.to_string());
        }
        let rows = query.fetch_all(&self.db).await?;
        let mut result = HashMap::with_capacity(rows.len());
        for row in &rows {
            let project_id = parse_id(row.try_get::<String, _>("project_id")?)?;
            let last_seen = parse_ts(row.try_get::<String, _>("last_seen")?)?;
            result.insert(project_id, last_seen);
        }
        Ok(result)
    }

    async fn override_fingerprint(&self, issue_id: Id, new_fingerprint: String) -> Result<Issue> {
        retry_write(&self.write, "override issue fingerprint", || {
            self.override_fingerprint_once(issue_id, &new_fingerprint)
        })
        .await
    }

    async fn delete(&self, id: Id) -> Result<()> {
        sqlx::query("DELETE FROM issues WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.db)
            .await?;
        Ok(())
    }
}

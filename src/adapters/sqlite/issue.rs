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

use super::Db;
use crate::domain::{Id, Issue, IssueStatus, Timestamp};
use crate::error::{Error, Result};
use crate::ports::{IssueFilter, IssueRepository, IssueSort, IssueUpsert, UpsertOutcome};
use async_trait::async_trait;
use sqlx::Row;
use std::str::FromStr;

#[derive(Clone)]
pub struct SqliteIssueRepository {
    db: Db,
}

impl SqliteIssueRepository {
    pub fn new(db: Db) -> Self {
        SqliteIssueRepository { db }
    }
}

const ISSUE_COLS: &str = "id, project_id, fingerprint, title, culprit, level, \
     environment, release, status, \
     first_seen, last_seen, event_count, created_at, updated_at";

fn row_to_issue(row: &sqlx::sqlite::SqliteRow) -> Result<Issue> {
    let status: String = row.try_get("status")?;
    Ok(Issue {
        id: parse_id(row.try_get::<String, _>("id")?)?,
        project_id: parse_id(row.try_get::<String, _>("project_id")?)?,
        fingerprint: row.try_get("fingerprint")?,
        title: row.try_get("title")?,
        culprit: row.try_get("culprit")?,
        level: row.try_get("level")?,
        environment: row.try_get("environment")?,
        release: row.try_get("release")?,
        status: IssueStatus::from_str(&status)?,
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

#[async_trait]
impl IssueRepository for SqliteIssueRepository {
    async fn find_by_id(&self, id: Id) -> Result<Option<Issue>> {
        let sql = format!("SELECT {ISSUE_COLS} FROM issues WHERE id = ?");
        let row = sqlx::query(&sql)
            .bind(id.to_string())
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
        let row = sqlx::query(&sql)
            .bind(project_id.to_string())
            .bind(fingerprint)
            .fetch_optional(&self.db)
            .await?;
        row.as_ref().map(row_to_issue).transpose()
    }

    async fn upsert_by_fingerprint(&self, upsert: IssueUpsert) -> Result<UpsertOutcome> {
        // Run in a transaction so the read-then-write is consistent under
        // concurrent ingestion of the same fingerprint.
        let mut tx = self.db.begin().await?;
        let seen = upsert.seen_at.to_rfc3339();

        let existing_sql =
            format!("SELECT {ISSUE_COLS} FROM issues WHERE project_id = ? AND fingerprint = ?");
        let existing = sqlx::query(&existing_sql)
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
            let update_sql = format!(
                "UPDATE issues SET \
                    status = ?, \
                    last_seen = ?, \
                    event_count = event_count + 1, \
                    updated_at = ? \
                 WHERE id = ? RETURNING {ISSUE_COLS}"
            );
            let updated_row = sqlx::query(&update_sql)
                .bind(new_status.as_str())
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
                (id, project_id, fingerprint, title, culprit, level, \
                 environment, release, status, \
                 first_seen, last_seen, event_count, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'unresolved', ?, ?, 1, ?, ?) \
             RETURNING {ISSUE_COLS}"
        );
        let inserted = sqlx::query(&insert_sql)
            .bind(id.to_string())
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

    async fn set_status(&self, issue_id: Id, status: IssueStatus) -> Result<Issue> {
        let sql = format!(
            "UPDATE issues SET status = ?, updated_at = ? WHERE id = ? RETURNING {ISSUE_COLS}"
        );
        let row = sqlx::query(&sql)
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
        let rows = sqlx::query(&sql)
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

    async fn override_fingerprint(&self, issue_id: Id, new_fingerprint: String) -> Result<Issue> {
        // One transaction so the collision check + merge/rename is atomic and
        // respects the unique (project_id, fingerprint) index under concurrency.
        let mut tx = self.db.begin().await?;
        let now = chrono::Utc::now().to_rfc3339();

        // 1. Load the target issue (the row identified by `issue_id` always
        //    survives — a merge deletes the *other* colliding row).
        let target_sql = format!("SELECT {ISSUE_COLS} FROM issues WHERE id = ?");
        let target_row = sqlx::query(&target_sql)
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
        let collision_row = sqlx::query(&collision_sql)
            .bind(target.project_id.to_string())
            .bind(&new_fingerprint)
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
            sqlx::query(&update_sql)
                .bind(&new_fingerprint)
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
            sqlx::query(&update_sql)
                .bind(&new_fingerprint)
                .bind(&now)
                .bind(target.id.to_string())
                .fetch_one(&mut *tx)
                .await?
        };

        let issue = row_to_issue(&updated_row)?;
        tx.commit().await?;
        Ok(issue)
    }

    async fn delete(&self, id: Id) -> Result<()> {
        sqlx::query("DELETE FROM issues WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.db)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> Db {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        crate::MIGRATOR.run(&pool).await.unwrap();
        pool
    }

    async fn seed_project(pool: &Db) -> Id {
        let team_id = Id::new_v4();
        let project_id = Id::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query("INSERT INTO teams (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind(team_id.to_string())
            .bind("t")
            .bind(&now)
            .bind(&now)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO projects (id, team_id, name, slug, dsn_public_key, retention_events, muted, created_at, updated_at) \
             VALUES (?, ?, 'P', 'p', 'dsn', 1000, 0, ?, ?)",
        )
        .bind(project_id.to_string())
        .bind(team_id.to_string())
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .unwrap();
        project_id
    }

    fn upsert(project_id: Id, fp: &str, at: Timestamp) -> IssueUpsert {
        IssueUpsert {
            project_id,
            fingerprint: fp.to_string(),
            title: format!("Error {fp}"),
            culprit: Some("main".into()),
            level: Some("error".into()),
            environment: None,
            release: None,
            seen_at: at,
        }
    }

    /// Like [`upsert`] but lets a test vary the first-class filterable fields
    /// (level / environment / release) so seeded rows differ.
    fn upsert_with(
        project_id: Id,
        fp: &str,
        at: Timestamp,
        level: Option<&str>,
        environment: Option<&str>,
        release: Option<&str>,
    ) -> IssueUpsert {
        IssueUpsert {
            level: level.map(str::to_string),
            environment: environment.map(str::to_string),
            release: release.map(str::to_string),
            ..upsert(project_id, fp, at)
        }
    }

    #[tokio::test]
    async fn first_upsert_is_new_then_increments() {
        let pool = pool().await;
        let project_id = seed_project(&pool).await;
        let repo = SqliteIssueRepository::new(pool);

        let t0 = chrono::Utc::now();
        let first = repo
            .upsert_by_fingerprint(upsert(project_id, "fp1", t0))
            .await
            .unwrap();
        assert!(first.is_new);
        assert!(!first.is_regression);
        assert_eq!(first.issue.event_count, 1);

        let t1 = t0 + chrono::Duration::seconds(5);
        let second = repo
            .upsert_by_fingerprint(upsert(project_id, "fp1", t1))
            .await
            .unwrap();
        assert!(!second.is_new, "same fingerprint must not be new");
        assert!(!second.is_regression);
        assert_eq!(
            second.issue.id, first.issue.id,
            "same fingerprint → same issue"
        );
        assert_eq!(second.issue.event_count, 2);
        assert!(second.issue.last_seen > first.issue.last_seen);
    }

    #[tokio::test]
    async fn upsert_on_resolved_issue_is_regression() {
        let pool = pool().await;
        let project_id = seed_project(&pool).await;
        let repo = SqliteIssueRepository::new(pool);

        let created = repo
            .upsert_by_fingerprint(upsert(project_id, "fp1", chrono::Utc::now()))
            .await
            .unwrap();
        repo.set_status(created.issue.id, IssueStatus::Resolved)
            .await
            .unwrap();

        let regressed = repo
            .upsert_by_fingerprint(upsert(project_id, "fp1", chrono::Utc::now()))
            .await
            .unwrap();
        assert!(!regressed.is_new);
        assert!(regressed.is_regression, "resolved → new event must regress");
        assert_eq!(regressed.issue.status, IssueStatus::Unresolved);
        assert_eq!(regressed.issue.event_count, 2);
    }

    #[tokio::test]
    async fn muted_issue_stays_muted_on_new_event() {
        let pool = pool().await;
        let project_id = seed_project(&pool).await;
        let repo = SqliteIssueRepository::new(pool);

        let created = repo
            .upsert_by_fingerprint(upsert(project_id, "fp1", chrono::Utc::now()))
            .await
            .unwrap();
        repo.set_status(created.issue.id, IssueStatus::Muted)
            .await
            .unwrap();

        let again = repo
            .upsert_by_fingerprint(upsert(project_id, "fp1", chrono::Utc::now()))
            .await
            .unwrap();
        assert!(!again.is_regression);
        assert_eq!(again.issue.status, IssueStatus::Muted);
        assert_eq!(
            again.issue.event_count, 2,
            "muted issue keeps counting events"
        );
    }

    #[tokio::test]
    async fn list_filters_by_status() {
        let pool = pool().await;
        let project_id = seed_project(&pool).await;
        let repo = SqliteIssueRepository::new(pool);

        let a = repo
            .upsert_by_fingerprint(upsert(project_id, "fpa", chrono::Utc::now()))
            .await
            .unwrap();
        repo.upsert_by_fingerprint(upsert(project_id, "fpb", chrono::Utc::now()))
            .await
            .unwrap();
        repo.set_status(a.issue.id, IssueStatus::Resolved)
            .await
            .unwrap();

        let unresolved = repo
            .list(
                project_id,
                IssueFilter {
                    status: Some(IssueStatus::Unresolved),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(unresolved.len(), 1);
        assert_eq!(unresolved[0].fingerprint, "fpb");

        let all = repo.list(project_id, IssueFilter::default()).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn list_filters_by_query() {
        let pool = pool().await;
        let project_id = seed_project(&pool).await;
        let repo = SqliteIssueRepository::new(pool);

        repo.upsert_by_fingerprint(upsert(project_id, "needle", chrono::Utc::now()))
            .await
            .unwrap();
        repo.upsert_by_fingerprint(upsert(project_id, "haystack", chrono::Utc::now()))
            .await
            .unwrap();

        // Titles are "Error needle" / "Error haystack".
        let hits = repo
            .list(
                project_id,
                IssueFilter {
                    query: Some("needle".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].fingerprint, "needle");
    }

    #[tokio::test]
    async fn list_orders_by_event_count() {
        let pool = pool().await;
        let project_id = seed_project(&pool).await;
        let repo = SqliteIssueRepository::new(pool);

        // "frequent" gets 3 events at older timestamps (event_count = 3).
        let base = chrono::Utc::now();
        for i in 0..3 {
            let at = base + chrono::Duration::seconds(i);
            repo.upsert_by_fingerprint(upsert(project_id, "frequent", at))
                .await
                .unwrap();
        }
        // "rare" gets a single event with the LATEST timestamp, so it would sort
        // first under LastSeen but must lose to "frequent" under EventCount.
        let latest = base + chrono::Duration::seconds(100);
        repo.upsert_by_fingerprint(upsert(project_id, "rare", latest))
            .await
            .unwrap();

        let by_count = repo
            .list(
                project_id,
                IssueFilter {
                    sort: IssueSort::EventCount,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(by_count.len(), 2);
        assert_eq!(
            by_count[0].fingerprint, "frequent",
            "higher event_count sorts first under EventCount"
        );
        assert_eq!(by_count[1].fingerprint, "rare");

        // Sanity: under the default LastSeen ordering, "rare" (latest) leads.
        let by_recency = repo.list(project_id, IssueFilter::default()).await.unwrap();
        assert_eq!(by_recency[0].fingerprint, "rare");
    }

    #[tokio::test]
    async fn first_upsert_persists_environment_and_release() {
        let pool = pool().await;
        let project_id = seed_project(&pool).await;
        let repo = SqliteIssueRepository::new(pool);

        let with_env = repo
            .upsert_by_fingerprint(upsert_with(
                project_id,
                "fp1",
                chrono::Utc::now(),
                Some("warning"),
                Some("production"),
                Some("1.2.3"),
            ))
            .await
            .unwrap();
        assert_eq!(with_env.issue.level.as_deref(), Some("warning"));
        assert_eq!(with_env.issue.environment.as_deref(), Some("production"));
        assert_eq!(with_env.issue.release.as_deref(), Some("1.2.3"));

        // Read back through find_by_id to confirm the columns round-trip.
        let read = repo.find_by_id(with_env.issue.id).await.unwrap().unwrap();
        assert_eq!(read.environment.as_deref(), Some("production"));
        assert_eq!(read.release.as_deref(), Some("1.2.3"));

        // A NULL environment/release decodes to None.
        let bare = repo
            .upsert_by_fingerprint(upsert_with(
                project_id,
                "fp2",
                chrono::Utc::now(),
                Some("error"),
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(bare.issue.environment, None);
        assert_eq!(bare.issue.release, None);
    }

    #[tokio::test]
    async fn list_filters_by_level() {
        let pool = pool().await;
        let project_id = seed_project(&pool).await;
        let repo = SqliteIssueRepository::new(pool);

        repo.upsert_by_fingerprint(upsert_with(
            project_id,
            "err",
            chrono::Utc::now(),
            Some("error"),
            None,
            None,
        ))
        .await
        .unwrap();
        repo.upsert_by_fingerprint(upsert_with(
            project_id,
            "warn",
            chrono::Utc::now(),
            Some("warning"),
            None,
            None,
        ))
        .await
        .unwrap();

        let only_warnings = repo
            .list(
                project_id,
                IssueFilter {
                    level: Some("warning".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(only_warnings.len(), 1);
        assert_eq!(only_warnings[0].fingerprint, "warn");

        let all = repo.list(project_id, IssueFilter::default()).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn list_filters_by_environment() {
        let pool = pool().await;
        let project_id = seed_project(&pool).await;
        let repo = SqliteIssueRepository::new(pool);

        repo.upsert_by_fingerprint(upsert_with(
            project_id,
            "prod",
            chrono::Utc::now(),
            Some("error"),
            Some("production"),
            None,
        ))
        .await
        .unwrap();
        repo.upsert_by_fingerprint(upsert_with(
            project_id,
            "staging",
            chrono::Utc::now(),
            Some("error"),
            Some("staging"),
            None,
        ))
        .await
        .unwrap();
        // A row with NULL environment must be excluded when the filter is set.
        repo.upsert_by_fingerprint(upsert_with(
            project_id,
            "noenv",
            chrono::Utc::now(),
            Some("error"),
            None,
            None,
        ))
        .await
        .unwrap();

        let prod = repo
            .list(
                project_id,
                IssueFilter {
                    environment: Some("production".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(prod.len(), 1, "NULL-environment rows are excluded");
        assert_eq!(prod[0].fingerprint, "prod");

        // With no environment filter, all three rows (incl. NULL) are returned.
        let all = repo.list(project_id, IssueFilter::default()).await.unwrap();
        assert_eq!(all.len(), 3);
    }

    #[tokio::test]
    async fn list_filters_by_release() {
        let pool = pool().await;
        let project_id = seed_project(&pool).await;
        let repo = SqliteIssueRepository::new(pool);

        repo.upsert_by_fingerprint(upsert_with(
            project_id,
            "v1",
            chrono::Utc::now(),
            Some("error"),
            None,
            Some("1.0.0"),
        ))
        .await
        .unwrap();
        repo.upsert_by_fingerprint(upsert_with(
            project_id,
            "v2",
            chrono::Utc::now(),
            Some("error"),
            None,
            Some("2.0.0"),
        ))
        .await
        .unwrap();

        let v2 = repo
            .list(
                project_id,
                IssueFilter {
                    release: Some("2.0.0".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(v2.len(), 1);
        assert_eq!(v2[0].fingerprint, "v2");
    }

    /// Insert a raw event row for an issue (mirrors the event.rs seed pattern)
    /// so override_fingerprint's event re-pointing can be asserted.
    async fn insert_event(pool: &Db, project_id: Id, issue_id: Id, at: Timestamp) -> Id {
        let id = Id::new_v4();
        sqlx::query(
            "INSERT INTO events (id, event_id, issue_id, project_id, payload, received_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(at.timestamp_millis().to_string())
        .bind(issue_id.to_string())
        .bind(project_id.to_string())
        .bind("{}")
        .bind(at.to_rfc3339())
        .execute(pool)
        .await
        .unwrap();
        id
    }

    async fn count_events_for_issue(pool: &Db, issue_id: Id) -> i64 {
        let row = sqlx::query("SELECT COUNT(*) AS n FROM events WHERE issue_id = ?")
            .bind(issue_id.to_string())
            .fetch_one(pool)
            .await
            .unwrap();
        row.try_get::<i64, _>("n").unwrap()
    }

    #[tokio::test]
    async fn override_fingerprint_renames_when_no_collision() {
        let pool = pool().await;
        let project_id = seed_project(&pool).await;
        let repo = SqliteIssueRepository::new(pool);

        let a = repo
            .upsert_by_fingerprint(upsert(project_id, "fpa", chrono::Utc::now()))
            .await
            .unwrap();

        let renamed = repo
            .override_fingerprint(a.issue.id, "renamed".to_string())
            .await
            .unwrap();

        // Same issue, new fingerprint.
        assert_eq!(renamed.id, a.issue.id);
        assert_eq!(renamed.fingerprint, "renamed");

        // The new fingerprint resolves to this issue; the old one is gone.
        let by_new = repo
            .find_by_fingerprint(project_id, "renamed")
            .await
            .unwrap();
        assert_eq!(by_new.map(|i| i.id), Some(a.issue.id));
        assert!(
            repo.find_by_fingerprint(project_id, "fpa")
                .await
                .unwrap()
                .is_none(),
            "old fingerprint must no longer resolve"
        );
    }

    #[tokio::test]
    async fn override_fingerprint_merges_into_existing_target() {
        let pool = pool().await;
        let project_id = seed_project(&pool).await;
        let repo = SqliteIssueRepository::new(pool.clone());

        // Issue A already holds the destination fingerprint "fpa"; it is the
        // collision the merge folds INTO the addressed issue, then deletes.
        // event_count = 2 (two upserts).
        let t0 = chrono::Utc::now();
        let a = repo
            .upsert_by_fingerprint(upsert(project_id, "fpa", t0))
            .await
            .unwrap();
        let t0b = t0 + chrono::Duration::seconds(5);
        repo.upsert_by_fingerprint(upsert(project_id, "fpa", t0b))
            .await
            .unwrap();
        let a = repo.find_by_id(a.issue.id).await.unwrap().unwrap();
        assert_eq!(a.event_count, 2);

        // Issue B is the addressed (target) issue; we override ITS fingerprint
        // to "fpa". Its first_seen is earlier than A's and its last_seen later
        // than A's, so the folded min/max is observable. event_count = 2.
        let t1_first = t0 - chrono::Duration::seconds(60);
        let b = repo
            .upsert_by_fingerprint(upsert(project_id, "fpb", t1_first))
            .await
            .unwrap();
        let t1_last = t0b + chrono::Duration::seconds(60);
        repo.upsert_by_fingerprint(upsert(project_id, "fpb", t1_last))
            .await
            .unwrap();
        let b = repo.find_by_id(b.issue.id).await.unwrap().unwrap();
        assert_eq!(b.event_count, 2);

        // Seed raw events for BOTH issues so re-pointing can be asserted.
        insert_event(&pool, project_id, a.id, t0).await;
        insert_event(&pool, project_id, a.id, t0b).await;
        insert_event(&pool, project_id, b.id, t1_first).await;
        insert_event(&pool, project_id, b.id, t1_last).await;
        assert_eq!(count_events_for_issue(&pool, a.id).await, 2);
        assert_eq!(count_events_for_issue(&pool, b.id).await, 2);

        // Override B's fingerprint to "fpa" — collides with A → MERGE. The
        // addressed issue (B) survives; the colliding source (A) is folded in
        // and deleted.
        let survivor = repo
            .override_fingerprint(b.id, "fpa".to_string())
            .await
            .unwrap();

        // The surviving issue is the addressed target (B) now holding "fpa".
        assert_eq!(survivor.id, b.id, "addressed issue B survives");
        assert_eq!(survivor.fingerprint, "fpa");
        assert_eq!(
            survivor.event_count,
            a.event_count + b.event_count,
            "event_count is summed (4)"
        );
        // Earliest first_seen (B's) and latest last_seen (B's) across both.
        assert_eq!(survivor.first_seen.timestamp(), b.first_seen.timestamp());
        assert_eq!(survivor.last_seen.timestamp(), b.last_seen.timestamp());

        // The colliding source A is deleted; only one issue with "fpa" remains.
        assert!(
            repo.find_by_id(a.id).await.unwrap().is_none(),
            "colliding source issue is deleted"
        );
        let all = repo.list(project_id, IssueFilter::default()).await.unwrap();
        assert_eq!(all.len(), 1, "exactly one issue remains after merge");

        // All events were re-pointed onto the survivor; none reference A.
        assert_eq!(count_events_for_issue(&pool, a.id).await, 0);
        assert_eq!(count_events_for_issue(&pool, survivor.id).await, 4);
    }

    #[tokio::test]
    async fn override_fingerprint_merge_adopts_source_extremes() {
        // Regression guard: the survivor must take the EARLIEST first_seen and the
        // LATEST last_seen across both issues even when those extremes belong to
        // the *source* (the deleted colliding issue), not the addressed survivor.
        // The sibling merge test seeds both extremes on the survivor, so it cannot
        // tell the min/max fold from "just keep the target's own timestamps".
        let pool = pool().await;
        let project_id = seed_project(&pool).await;
        let repo = SqliteIssueRepository::new(pool);

        let t0 = chrono::Utc::now();

        // Source A ("fpa") spans the WIDER window: first_seen = t0-60s (earliest of
        // all four events), last_seen = t0+60s (latest). event_count = 2.
        let a = repo
            .upsert_by_fingerprint(upsert(
                project_id,
                "fpa",
                t0 - chrono::Duration::seconds(60),
            ))
            .await
            .unwrap();
        repo.upsert_by_fingerprint(upsert(
            project_id,
            "fpa",
            t0 + chrono::Duration::seconds(60),
        ))
        .await
        .unwrap();
        let a = repo.find_by_id(a.issue.id).await.unwrap().unwrap();

        // Target/survivor B ("fpb") sits entirely INSIDE A's window: first_seen =
        // t0, last_seen = t0+5s. event_count = 2.
        let b = repo
            .upsert_by_fingerprint(upsert(project_id, "fpb", t0))
            .await
            .unwrap();
        repo.upsert_by_fingerprint(upsert(project_id, "fpb", t0 + chrono::Duration::seconds(5)))
            .await
            .unwrap();
        let b = repo.find_by_id(b.issue.id).await.unwrap().unwrap();

        // Override B → "fpa": collides with A → MERGE; the addressed issue B
        // survives and the source A is folded in and deleted.
        let survivor = repo
            .override_fingerprint(b.id, "fpa".to_string())
            .await
            .unwrap();

        assert_eq!(survivor.id, b.id, "addressed issue B survives");
        assert_eq!(
            survivor.event_count,
            a.event_count + b.event_count,
            "event_count is summed (4)"
        );
        // The extremes come from the SOURCE (A), not the survivor (B) — exactly the
        // case the sibling merge test does not exercise.
        assert_eq!(
            survivor.first_seen.timestamp(),
            a.first_seen.timestamp(),
            "survivor adopts the source's earlier first_seen"
        );
        assert_eq!(
            survivor.last_seen.timestamp(),
            a.last_seen.timestamp(),
            "survivor adopts the source's later last_seen"
        );
        assert!(
            repo.find_by_id(a.id).await.unwrap().is_none(),
            "colliding source issue is deleted"
        );
    }

    #[tokio::test]
    async fn override_fingerprint_noop_when_unchanged() {
        let pool = pool().await;
        let project_id = seed_project(&pool).await;
        let repo = SqliteIssueRepository::new(pool);

        let a = repo
            .upsert_by_fingerprint(upsert(project_id, "fpa", chrono::Utc::now()))
            .await
            .unwrap();

        let same = repo
            .override_fingerprint(a.issue.id, "fpa".to_string())
            .await
            .unwrap();
        assert_eq!(same.id, a.issue.id);
        assert_eq!(same.fingerprint, "fpa");
        assert_eq!(same.event_count, a.issue.event_count);
    }

    #[tokio::test]
    async fn list_composes_status_level_environment_release() {
        let pool = pool().await;
        let project_id = seed_project(&pool).await;
        let repo = SqliteIssueRepository::new(pool);

        // The matching row: error / production / 1.0.0, left unresolved.
        repo.upsert_by_fingerprint(upsert_with(
            project_id,
            "match",
            chrono::Utc::now(),
            Some("error"),
            Some("production"),
            Some("1.0.0"),
        ))
        .await
        .unwrap();
        // Same env/release/level but resolved → excluded by the status filter.
        let resolved = repo
            .upsert_by_fingerprint(upsert_with(
                project_id,
                "resolved",
                chrono::Utc::now(),
                Some("error"),
                Some("production"),
                Some("1.0.0"),
            ))
            .await
            .unwrap();
        repo.set_status(resolved.issue.id, IssueStatus::Resolved)
            .await
            .unwrap();
        // Differing release → excluded by the release filter.
        repo.upsert_by_fingerprint(upsert_with(
            project_id,
            "otherrel",
            chrono::Utc::now(),
            Some("error"),
            Some("production"),
            Some("2.0.0"),
        ))
        .await
        .unwrap();

        let hits = repo
            .list(
                project_id,
                IssueFilter {
                    status: Some(IssueStatus::Unresolved),
                    level: Some("error".into()),
                    environment: Some("production".into()),
                    release: Some("1.0.0".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(hits.len(), 1, "all sentinels compose to a single match");
        assert_eq!(hits[0].fingerprint, "match");
    }
}

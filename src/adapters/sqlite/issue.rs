//! SQLite [`IssueRepository`] adapter.
//!
//! Persistence notes (mirroring `migrations/0001_init.sql`):
//!   * `id` / `project_id` stored as TEXT (UUID v4 string form).
//!   * Timestamps TEXT in RFC3339/ISO-8601 (UTC).
//!   * `status` TEXT (`unresolved` | `resolved` | `muted`).
//!
//! Grouping & regression logic (MVP §7, §8.1) lives in
//! [`upsert_by_fingerprint`]: one issue per `(project_id, fingerprint)`; a new
//! event on a `resolved` issue flips it back to `unresolved` (regression).
//! SQL is ANSI-friendly; the only SQLite-specific bit is TEXT row decoding.

use super::Db;
use crate::domain::{Id, Issue, IssueStatus, Timestamp};
use crate::error::{Error, Result};
use crate::ports::{IssueFilter, IssueRepository, IssueUpsert, UpsertOutcome};
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

const ISSUE_COLS: &str = "id, project_id, fingerprint, title, culprit, level, status, \
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
            // keeps suppressing notifications (§8.1). last_seen always advances.
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

        // New fingerprint → brand-new unresolved issue (§7 new-issue notice).
        let id = Id::new_v4();
        let insert_sql = format!(
            "INSERT INTO issues \
                (id, project_id, fingerprint, title, culprit, level, status, \
                 first_seen, last_seen, event_count, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, 'unresolved', ?, ?, 1, ?, ?) \
             RETURNING {ISSUE_COLS}"
        );
        let inserted = sqlx::query(&insert_sql)
            .bind(id.to_string())
            .bind(upsert.project_id.to_string())
            .bind(&upsert.fingerprint)
            .bind(&upsert.title)
            .bind(&upsert.culprit)
            .bind(&upsert.level)
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
        // Optional status + text-search filters. We bind a sentinel for the
        // optional pieces so the SQL text stays static and ANSI-friendly:
        //   * status: NULL → match all
        //   * query:  NULL → match all; otherwise LIKE on title/culprit
        let like = filter.query.as_ref().map(|q| format!("%{q}%"));
        let limit = filter.limit.unwrap_or(50).clamp(1, 500);
        let offset = filter.offset.unwrap_or(0).max(0);

        let sql = format!(
            "SELECT {ISSUE_COLS} FROM issues \
             WHERE project_id = ? \
               AND (? IS NULL OR status = ?) \
               AND (? IS NULL OR title LIKE ? OR culprit LIKE ?) \
             ORDER BY last_seen DESC \
             LIMIT ? OFFSET ?"
        );
        let status_str = filter.status.map(|s| s.as_str().to_string());
        let rows = sqlx::query(&sql)
            .bind(project_id.to_string())
            .bind(&status_str)
            .bind(&status_str)
            .bind(&like)
            .bind(&like)
            .bind(&like)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.db)
            .await?;
        rows.iter().map(row_to_issue).collect()
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
            seen_at: at,
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
}

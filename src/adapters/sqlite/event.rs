//! SQLite [`EventRepository`] adapter.
//!
//! Persistence notes (mirroring `migrations/0001_init.sql`):
//!   * `id` / `issue_id` / `project_id` stored as TEXT (UUID v4 string form).
//!   * `received_at` TEXT in RFC3339/ISO-8601 (UTC) — string-sortable.
//!   * `payload` is the full event JSON, stored as TEXT.
//!
//! Retention (MVP §13) lives in [`prune_events_over_retention`]: keep at most
//! N most-recent events per project, deleting older ones. Aggregate counters on
//! the issue are preserved (they live in a different table). SQL is
//! ANSI-friendly; the only SQLite-specific bit is TEXT row decoding.

use super::Db;
use crate::domain::{Event, Id, Timestamp};
use crate::error::{Error, Result};
use crate::ports::{EventRepository, NewEvent};
use async_trait::async_trait;
use sqlx::Row;

#[derive(Clone)]
pub struct SqliteEventRepository {
    db: Db,
}

impl SqliteEventRepository {
    pub fn new(db: Db) -> Self {
        SqliteEventRepository { db }
    }
}

const EVENT_COLS: &str = "id, event_id, issue_id, project_id, payload, received_at";

fn row_to_event(row: &sqlx::sqlite::SqliteRow) -> Result<Event> {
    let payload_text: String = row.try_get("payload")?;
    let payload = serde_json::from_str(&payload_text)?;
    Ok(Event {
        id: parse_id(row.try_get::<String, _>("id")?)?,
        event_id: row.try_get("event_id")?,
        issue_id: parse_id(row.try_get::<String, _>("issue_id")?)?,
        project_id: parse_id(row.try_get::<String, _>("project_id")?)?,
        payload,
        received_at: parse_ts(row.try_get::<String, _>("received_at")?)?,
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
impl EventRepository for SqliteEventRepository {
    async fn insert(&self, new: NewEvent) -> Result<Event> {
        let id = Id::new_v4();
        let payload_text = serde_json::to_string(&new.payload)?;
        let sql = format!(
            "INSERT INTO events (id, event_id, issue_id, project_id, payload, received_at) \
             VALUES (?, ?, ?, ?, ?, ?) RETURNING {EVENT_COLS}"
        );
        let row = sqlx::query(&sql)
            .bind(id.to_string())
            .bind(new.event_id)
            .bind(new.issue_id.to_string())
            .bind(new.project_id.to_string())
            .bind(payload_text)
            .bind(new.received_at.to_rfc3339())
            .fetch_one(&self.db)
            .await?;
        row_to_event(&row)
    }

    async fn find_by_id(&self, id: Id) -> Result<Option<Event>> {
        let sql = format!("SELECT {EVENT_COLS} FROM events WHERE id = ?");
        let row = sqlx::query(&sql)
            .bind(id.to_string())
            .fetch_optional(&self.db)
            .await?;
        row.as_ref().map(row_to_event).transpose()
    }

    async fn recent_events(&self, issue_id: Id, limit: i64) -> Result<Vec<Event>> {
        let limit = limit.clamp(1, 500);
        // Secondary sort on id makes ordering deterministic when timestamps tie.
        let sql = format!(
            "SELECT {EVENT_COLS} FROM events WHERE issue_id = ? \
             ORDER BY received_at DESC, id DESC LIMIT ?"
        );
        let rows = sqlx::query(&sql)
            .bind(issue_id.to_string())
            .bind(limit)
            .fetch_all(&self.db)
            .await?;
        rows.iter().map(row_to_event).collect()
    }

    async fn latest_for_issue(&self, issue_id: Id) -> Result<Option<Event>> {
        let sql = format!(
            "SELECT {EVENT_COLS} FROM events WHERE issue_id = ? \
             ORDER BY received_at DESC, id DESC LIMIT 1"
        );
        let row = sqlx::query(&sql)
            .bind(issue_id.to_string())
            .fetch_optional(&self.db)
            .await?;
        row.as_ref().map(row_to_event).transpose()
    }

    async fn count_for_project(&self, project_id: Id) -> Result<i64> {
        let row = sqlx::query("SELECT COUNT(*) AS n FROM events WHERE project_id = ?")
            .bind(project_id.to_string())
            .fetch_one(&self.db)
            .await?;
        Ok(row.try_get::<i64, _>("n")?)
    }

    async fn prune_events_over_retention(
        &self,
        project_id: Id,
        retention_events: i64,
    ) -> Result<u64> {
        // Keep the N most-recent events for the project; delete the rest (§13).
        // ANSI-friendly: delete rows not in the "keep" set selected by recency.
        // `received_at DESC, id DESC` matches the read-side ordering so the kept
        // set is exactly what the UI shows. A non-positive retention keeps none.
        let keep = retention_events.max(0);
        let sql = "DELETE FROM events \
             WHERE project_id = ? \
               AND id NOT IN ( \
                   SELECT id FROM events \
                   WHERE project_id = ? \
                   ORDER BY received_at DESC, id DESC \
                   LIMIT ? \
               )";
        let res = sqlx::query(sql)
            .bind(project_id.to_string())
            .bind(project_id.to_string())
            .bind(keep)
            .execute(&self.db)
            .await?;
        Ok(res.rows_affected())
    }

    async fn project_ids_with_events(&self) -> Result<Vec<Id>> {
        let rows = sqlx::query("SELECT DISTINCT project_id FROM events")
            .fetch_all(&self.db)
            .await?;
        rows.iter()
            .map(|r| parse_id(r.try_get::<String, _>("project_id")?))
            .collect()
    }

    async fn delete_older_than(&self, project_id: Id, cutoff: Timestamp) -> Result<u64> {
        // received_at is RFC3339 TEXT; lexical comparison equals chronological
        // comparison for fixed-offset UTC strings produced by `to_rfc3339`.
        let res = sqlx::query("DELETE FROM events WHERE project_id = ? AND received_at < ?")
            .bind(project_id.to_string())
            .bind(cutoff.to_rfc3339())
            .execute(&self.db)
            .await?;
        Ok(res.rows_affected())
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

    /// Seed a project + a single issue, returning (project_id, issue_id).
    async fn seed(pool: &Db) -> (Id, Id) {
        let team_id = Id::new_v4();
        let project_id = Id::new_v4();
        let issue_id = Id::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        // Derive unique name/slug/dsn from the ids so seed() can be called more
        // than once per pool without tripping UNIQUE constraints.
        let team_name = format!("team-{team_id}");
        let project_slug = format!("project-{project_id}");
        let project_dsn = format!("dsn-{project_id}");
        sqlx::query("INSERT INTO teams (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind(team_id.to_string())
            .bind(&team_name)
            .bind(&now)
            .bind(&now)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO projects (id, team_id, name, slug, dsn_public_key, retention_events, muted, created_at, updated_at) \
             VALUES (?, ?, 'P', ?, ?, 1000, 0, ?, ?)",
        )
        .bind(project_id.to_string())
        .bind(team_id.to_string())
        .bind(&project_slug)
        .bind(&project_dsn)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO issues (id, project_id, fingerprint, title, status, first_seen, last_seen, event_count, created_at, updated_at) \
             VALUES (?, ?, 'fp', 'T', 'unresolved', ?, ?, 0, ?, ?)",
        )
        .bind(issue_id.to_string())
        .bind(project_id.to_string())
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .unwrap();
        (project_id, issue_id)
    }

    fn new_event(project_id: Id, issue_id: Id, at: Timestamp) -> NewEvent {
        NewEvent {
            event_id: at.timestamp_millis().to_string(),
            issue_id,
            project_id,
            payload: serde_json::json!({ "message": "boom", "ts": at.to_rfc3339() }),
            received_at: at,
        }
    }

    #[tokio::test]
    async fn insert_roundtrips_payload() {
        let pool = pool().await;
        let (project_id, issue_id) = seed(&pool).await;
        let repo = SqliteEventRepository::new(pool);

        let inserted = repo
            .insert(new_event(project_id, issue_id, chrono::Utc::now()))
            .await
            .unwrap();
        let fetched = repo.find_by_id(inserted.id).await.unwrap().unwrap();
        assert_eq!(fetched.payload["message"], "boom");
        assert_eq!(fetched.project_id, project_id);
        assert_eq!(fetched.issue_id, issue_id);
    }

    #[tokio::test]
    async fn prune_keeps_n_most_recent_per_project() {
        let pool = pool().await;
        let (project_id, issue_id) = seed(&pool).await;
        let repo = SqliteEventRepository::new(pool);

        let base = chrono::Utc::now();
        // Insert 5 events with strictly increasing timestamps.
        for i in 0..5 {
            let at = base + chrono::Duration::seconds(i);
            repo.insert(new_event(project_id, issue_id, at))
                .await
                .unwrap();
        }
        assert_eq!(repo.count_for_project(project_id).await.unwrap(), 5);

        // Keep the 2 most-recent → deletes 3.
        let deleted = repo
            .prune_events_over_retention(project_id, 2)
            .await
            .unwrap();
        assert_eq!(deleted, 3);
        assert_eq!(repo.count_for_project(project_id).await.unwrap(), 2);

        // The survivors are the newest two (seconds 3 and 4).
        let remaining = repo.recent_events(issue_id, 100).await.unwrap();
        assert_eq!(remaining.len(), 2);
        let newest = base + chrono::Duration::seconds(4);
        let second = base + chrono::Duration::seconds(3);
        assert_eq!(remaining[0].received_at.timestamp(), newest.timestamp());
        assert_eq!(remaining[1].received_at.timestamp(), second.timestamp());
    }

    #[tokio::test]
    async fn prune_is_per_project_and_noop_when_under_limit() {
        let pool = pool().await;
        let (project_a, issue_a) = seed(&pool).await;
        let (project_b, issue_b) = seed(&pool).await;
        let repo = SqliteEventRepository::new(pool);

        let base = chrono::Utc::now();
        for i in 0..3 {
            let at = base + chrono::Duration::seconds(i);
            repo.insert(new_event(project_a, issue_a, at))
                .await
                .unwrap();
        }
        repo.insert(new_event(project_b, issue_b, base))
            .await
            .unwrap();

        // Pruning project A to 2 must not touch project B.
        let deleted = repo
            .prune_events_over_retention(project_a, 2)
            .await
            .unwrap();
        assert_eq!(deleted, 1);
        assert_eq!(repo.count_for_project(project_a).await.unwrap(), 2);
        assert_eq!(repo.count_for_project(project_b).await.unwrap(), 1);

        // Retention above the current count is a no-op.
        let deleted = repo
            .prune_events_over_retention(project_a, 100)
            .await
            .unwrap();
        assert_eq!(deleted, 0);
        assert_eq!(repo.count_for_project(project_a).await.unwrap(), 2);
    }

    #[tokio::test]
    async fn project_ids_with_events_is_distinct() {
        let pool = pool().await;
        let (project_a, issue_a) = seed(&pool).await;
        let (project_b, issue_b) = seed(&pool).await;
        let repo = SqliteEventRepository::new(pool);

        let now = chrono::Utc::now();
        repo.insert(new_event(project_a, issue_a, now))
            .await
            .unwrap();
        repo.insert(new_event(project_a, issue_a, now))
            .await
            .unwrap();
        repo.insert(new_event(project_b, issue_b, now))
            .await
            .unwrap();

        let mut ids = repo.project_ids_with_events().await.unwrap();
        ids.sort();
        let mut expected = vec![project_a, project_b];
        expected.sort();
        assert_eq!(ids, expected);
    }

    #[tokio::test]
    async fn delete_older_than_cutoff() {
        let pool = pool().await;
        let (project_id, issue_id) = seed(&pool).await;
        let repo = SqliteEventRepository::new(pool);

        let base = chrono::Utc::now();
        for i in 0..4 {
            let at = base + chrono::Duration::seconds(i);
            repo.insert(new_event(project_id, issue_id, at))
                .await
                .unwrap();
        }
        let cutoff = base + chrono::Duration::seconds(2);
        let deleted = repo.delete_older_than(project_id, cutoff).await.unwrap();
        assert_eq!(deleted, 2, "seconds 0 and 1 are strictly before cutoff");
        assert_eq!(repo.count_for_project(project_id).await.unwrap(), 2);
    }
}

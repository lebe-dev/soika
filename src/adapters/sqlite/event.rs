//! SQLite [`EventRepository`] adapter.
//!
//! Persistence notes (mirroring `migrations/0001_init.sql`):
//!   * `id` / `issue_id` / `project_id` stored as TEXT (UUID v4 string form).
//!   * `received_at` TEXT in RFC3339/ISO-8601 (UTC) — string-sortable.
//!   * `payload` is the full event JSON, stored as TEXT.
//!
//! Retention lives in [`prune_events_over_retention`]: keep at most
//! N most-recent events per project, deleting older ones. Aggregate counters on
//! the issue are preserved (they live in a different table). SQL is
//! ANSI-friendly; the only SQLite-specific bit is TEXT row decoding.

use super::{Db, retry_write};
use crate::config::WritePolicy;
use crate::domain::{Event, Id, Timestamp};
use crate::error::{Error, Result};
use crate::ports::{EventRepository, NewEvent};
use async_trait::async_trait;
use sqlx::Row;

#[derive(Clone)]
pub struct SqliteEventRepository {
    db: Db,
    write: WritePolicy,
}

impl SqliteEventRepository {
    pub fn new(db: Db) -> Self {
        SqliteEventRepository {
            db,
            write: WritePolicy::default(),
        }
    }

    /// Override the write retry / delete-batch policy (wired from `DB_WRITE_*` /
    /// `DB_DELETE_BATCH` in `soika::build_state`); [`new`](Self::new) uses the
    /// defaults.
    pub fn with_write_policy(mut self, write: WritePolicy) -> Self {
        self.write = write;
        self
    }

    /// Delete rows in [`WritePolicy::delete_batch`]-sized chunks until a chunk
    /// comes back short, returning the total number of rows deleted.
    ///
    /// Retention can span tens of thousands of rows; as a single `DELETE` it
    /// would hold SQLite's one write lock for the whole sweep and starve
    /// ingestion. `run_batch` executes exactly one chunk (bounded by the `LIMIT`
    /// its caller binds), so the lock is released between chunks and each chunk
    /// is retried independently on contention.
    async fn delete_in_batches<F, Fut>(&self, what: &str, mut run_batch: F) -> Result<u64>
    where
        F: FnMut(i64) -> Fut,
        Fut: Future<Output = Result<u64>>,
    {
        let batch = self.write.delete_batch;
        let mut total = 0u64;
        loop {
            let deleted = retry_write(&self.write, what, || run_batch(batch)).await?;
            total += deleted;
            if deleted < batch as u64 {
                return Ok(total);
            }
            // Hand the write lock (and the runtime) to any waiting ingest task
            // before taking it again for the next chunk.
            tokio::task::yield_now().await;
        }
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
        let received_at = new.received_at.to_rfc3339();
        // One statement, so re-running it after a lock conflict is safe: either
        // the row was written (Ok) or nothing was.
        let row = retry_write(&self.write, "insert event", || async {
            let row = sqlx::query(sqlx::AssertSqlSafe(&*sql))
                .bind(id.to_string())
                .bind(&new.event_id)
                .bind(new.issue_id.to_string())
                .bind(new.project_id.to_string())
                .bind(&payload_text)
                .bind(&received_at)
                .fetch_one(&self.db)
                .await?;
            Ok(row)
        })
        .await?;
        row_to_event(&row)
    }

    async fn find_by_id(&self, id: Id) -> Result<Option<Event>> {
        let sql = format!("SELECT {EVENT_COLS} FROM events WHERE id = ?");
        let row = sqlx::query(sqlx::AssertSqlSafe(&*sql))
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
        let rows = sqlx::query(sqlx::AssertSqlSafe(&*sql))
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
        let row = sqlx::query(sqlx::AssertSqlSafe(&*sql))
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

    async fn count_in_window(&self, issue_id: Id, since: Timestamp) -> Result<i64> {
        // received_at is RFC3339 TEXT; lexical comparison equals chronological
        // comparison for fixed-offset UTC strings (mirrors `delete_older_than`).
        let row =
            sqlx::query("SELECT COUNT(*) AS n FROM events WHERE issue_id = ? AND received_at >= ?")
                .bind(issue_id.to_string())
                .bind(since.to_rfc3339())
                .fetch_one(&self.db)
                .await?;
        Ok(row.try_get::<i64, _>("n")?)
    }

    async fn prune_events_over_retention(
        &self,
        project_id: Id,
        retention_events: i64,
    ) -> Result<u64> {
        // Keep the N most-recent events for the project; delete the rest, in
        // batches (see `delete_in_batches`). ANSI-friendly: select the rows *past*
        // the keep window by recency (`LIMIT batch OFFSET keep`) and delete those.
        // `received_at DESC, id DESC` matches the read-side ordering so the kept
        // set is exactly what the UI shows. A non-positive retention keeps none.
        let keep = retention_events.max(0);
        let sql = "DELETE FROM events \
             WHERE id IN ( \
                   SELECT id FROM events \
                   WHERE project_id = ? \
                   ORDER BY received_at DESC, id DESC \
                   LIMIT ? OFFSET ? \
               )";
        self.delete_in_batches("prune events over retention", |batch| async move {
            let res = sqlx::query(sql)
                .bind(project_id.to_string())
                .bind(batch)
                .bind(keep)
                .execute(&self.db)
                .await?;
            Ok(res.rows_affected())
        })
        .await
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
        // Batched (see `delete_in_batches`) so an age-based sweep of a large
        // backlog does not hold the write lock against ingestion.
        let sql = "DELETE FROM events \
             WHERE id IN ( \
                   SELECT id FROM events \
                   WHERE project_id = ? AND received_at < ? \
                   LIMIT ? \
               )";
        let cutoff = cutoff.to_rfc3339();
        self.delete_in_batches("delete events older than cutoff", |batch| {
            let cutoff = cutoff.clone();
            async move {
                let res = sqlx::query(sql)
                    .bind(project_id.to_string())
                    .bind(cutoff)
                    .bind(batch)
                    .execute(&self.db)
                    .await?;
                Ok(res.rows_affected())
            }
        })
        .await
    }
}

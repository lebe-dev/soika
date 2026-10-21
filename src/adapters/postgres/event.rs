//! PostgreSQL [`EventRepository`] adapter.
//!
//! `payload` is the event JSON stored as TEXT (see `migrations/postgres`).
//! Retention keeps at most N most-recent events per project; aggregate counters
//! on the issue are preserved (they live in a different table).

use super::{Db, ts};
use crate::adapters::retry::retry_write;
use crate::config::WritePolicy;
use crate::domain::{Event, Id, Timestamp};
use crate::error::Result;
use crate::ports::{EventRepository, NewEvent};
use async_trait::async_trait;
use sqlx::{AssertSqlSafe, FromRow};

#[derive(Clone)]
pub struct PgEventRepository {
    db: Db,
    write: WritePolicy,
}

impl PgEventRepository {
    pub fn new(db: Db) -> Self {
        PgEventRepository {
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
    /// Postgres has no single write lock to hand back, but chunking still keeps
    /// each transaction (and the row locks / WAL it produces) small, so a large
    /// retention sweep never stalls ingestion behind one huge DELETE.
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
            tokio::task::yield_now().await;
        }
    }
}

const EVENT_COLS: &str = "id, event_id, issue_id, project_id, payload, received_at";

#[derive(FromRow)]
struct EventRow {
    id: Id,
    event_id: String,
    issue_id: Id,
    project_id: Id,
    payload: String,
    received_at: Timestamp,
}

impl EventRow {
    fn into_domain(self) -> Result<Event> {
        Ok(Event {
            id: self.id,
            event_id: self.event_id,
            issue_id: self.issue_id,
            project_id: self.project_id,
            payload: serde_json::from_str(&self.payload)?,
            received_at: self.received_at,
        })
    }
}

#[async_trait]
impl EventRepository for PgEventRepository {
    async fn insert(&self, new: NewEvent) -> Result<Event> {
        let id = Id::new_v4();
        let payload_text = serde_json::to_string(&new.payload)?;
        let sql = format!(
            "INSERT INTO events (id, event_id, issue_id, project_id, payload, received_at) \
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING {EVENT_COLS}"
        );
        let received_at = ts(new.received_at);
        // One statement, so re-running it after a transient conflict is safe:
        // either the row was written (Ok) or nothing was.
        let row = retry_write(&self.write, "insert event", || async {
            let row = sqlx::query_as::<_, EventRow>(AssertSqlSafe(&*sql))
                .bind(id)
                .bind(&new.event_id)
                .bind(new.issue_id)
                .bind(new.project_id)
                .bind(&payload_text)
                .bind(received_at)
                .fetch_one(&self.db)
                .await?;
            Ok(row)
        })
        .await?;
        row.into_domain()
    }

    async fn find_by_id(&self, id: Id) -> Result<Option<Event>> {
        let row = sqlx::query_as::<_, EventRow>(AssertSqlSafe(format!(
            "SELECT {EVENT_COLS} FROM events WHERE id = $1"
        )))
        .bind(id)
        .fetch_optional(&self.db)
        .await?;
        row.map(EventRow::into_domain).transpose()
    }

    async fn recent_events(&self, issue_id: Id, limit: i64) -> Result<Vec<Event>> {
        let limit = limit.clamp(1, 500);
        // Secondary sort on id makes ordering deterministic when timestamps tie.
        let rows = sqlx::query_as::<_, EventRow>(AssertSqlSafe(format!(
            "SELECT {EVENT_COLS} FROM events WHERE issue_id = $1 \
             ORDER BY received_at DESC, id DESC LIMIT $2"
        )))
        .bind(issue_id)
        .bind(limit)
        .fetch_all(&self.db)
        .await?;
        rows.into_iter().map(EventRow::into_domain).collect()
    }

    async fn latest_for_issue(&self, issue_id: Id) -> Result<Option<Event>> {
        let row = sqlx::query_as::<_, EventRow>(AssertSqlSafe(format!(
            "SELECT {EVENT_COLS} FROM events WHERE issue_id = $1 \
             ORDER BY received_at DESC, id DESC LIMIT 1"
        )))
        .bind(issue_id)
        .fetch_optional(&self.db)
        .await?;
        row.map(EventRow::into_domain).transpose()
    }

    async fn count_for_project(&self, project_id: Id) -> Result<i64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE project_id = $1")
            .bind(project_id)
            .fetch_one(&self.db)
            .await?;
        Ok(count)
    }

    async fn count_in_window(&self, issue_id: Id, since: Timestamp) -> Result<i64> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM events WHERE issue_id = $1 AND received_at >= $2",
        )
        .bind(issue_id)
        .bind(since)
        .fetch_one(&self.db)
        .await?;
        Ok(count)
    }

    async fn prune_events_over_retention(
        &self,
        project_id: Id,
        retention_events: i64,
    ) -> Result<u64> {
        // Keep the N most-recent events for the project; delete the rest, in
        // batches: select the rows *past* the keep window by recency
        // (`LIMIT batch OFFSET keep`) and delete those. `received_at DESC, id
        // DESC` matches the read-side ordering so the kept set is exactly what
        // the UI shows. A non-positive retention keeps none.
        let keep = retention_events.max(0);
        let sql = "DELETE FROM events \
             WHERE id IN ( \
                   SELECT id FROM events \
                   WHERE project_id = $1 \
                   ORDER BY received_at DESC, id DESC \
                   LIMIT $2 OFFSET $3 \
               )";
        self.delete_in_batches("prune events over retention", |batch| async move {
            let res = sqlx::query(sql)
                .bind(project_id)
                .bind(batch)
                .bind(keep)
                .execute(&self.db)
                .await?;
            Ok(res.rows_affected())
        })
        .await
    }

    async fn project_ids_with_events(&self) -> Result<Vec<Id>> {
        let ids = sqlx::query_scalar("SELECT DISTINCT project_id FROM events")
            .fetch_all(&self.db)
            .await?;
        Ok(ids)
    }

    async fn delete_older_than(&self, project_id: Id, cutoff: Timestamp) -> Result<u64> {
        let sql = "DELETE FROM events \
             WHERE id IN ( \
                   SELECT id FROM events \
                   WHERE project_id = $1 AND received_at < $2 \
                   LIMIT $3 \
               )";
        self.delete_in_batches("delete events older than cutoff", |batch| async move {
            let res = sqlx::query(sql)
                .bind(project_id)
                .bind(cutoff)
                .bind(batch)
                .execute(&self.db)
                .await?;
            Ok(res.rows_affected())
        })
        .await
    }
}

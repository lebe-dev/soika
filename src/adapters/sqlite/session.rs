//! SQLite [`SessionRepository`] adapter.

use super::{Db, id_from_db, id_to_db, ts_from_db, ts_to_db};
use crate::domain::{Id, Session, Timestamp};
use crate::error::Result;
use crate::ports::SessionRepository;
use async_trait::async_trait;
use sqlx::FromRow;

#[derive(Clone)]
pub struct SqliteSessionRepository {
    db: Db,
}

impl SqliteSessionRepository {
    pub fn new(db: Db) -> Self {
        SqliteSessionRepository { db }
    }
}

/// Raw row mirroring the `sessions` table.
#[derive(FromRow)]
struct SessionRow {
    id: String,
    user_id: String,
    created_at: String,
    expires_at: String,
}

impl SessionRow {
    fn into_domain(self) -> Result<Session> {
        Ok(Session {
            id: self.id,
            user_id: id_from_db(&self.user_id)?,
            created_at: ts_from_db(&self.created_at)?,
            expires_at: ts_from_db(&self.expires_at)?,
        })
    }
}

#[async_trait]
impl SessionRepository for SqliteSessionRepository {
    async fn create(&self, id: String, user_id: Id, expires_at: Timestamp) -> Result<Session> {
        let now: Timestamp = chrono::Utc::now();
        sqlx::query(
            "INSERT INTO sessions (id, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(id_to_db(user_id))
        .bind(ts_to_db(now))
        .bind(ts_to_db(expires_at))
        .execute(&self.db)
        .await?;

        Ok(Session {
            id,
            user_id,
            created_at: now,
            expires_at,
        })
    }

    async fn find(&self, id: &str) -> Result<Option<Session>> {
        // Expired sessions are treated as absent; comparison is on the
        // RFC3339 TEXT form, which sorts lexicographically in UTC.
        let now = ts_to_db(chrono::Utc::now());
        let row = sqlx::query_as::<_, SessionRow>(
            "SELECT id, user_id, created_at, expires_at FROM sessions \
             WHERE id = ? AND expires_at > ?",
        )
        .bind(id)
        .bind(now)
        .fetch_optional(&self.db)
        .await?;
        row.map(SessionRow::into_domain).transpose()
    }

    async fn delete(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(id)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    async fn delete_for_user(&self, user_id: Id) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE user_id = ?")
            .bind(id_to_db(user_id))
            .execute(&self.db)
            .await?;
        Ok(())
    }

    async fn delete_expired(&self, now: Timestamp) -> Result<u64> {
        let result = sqlx::query("DELETE FROM sessions WHERE expires_at <= ?")
            .bind(ts_to_db(now))
            .execute(&self.db)
            .await?;
        Ok(result.rows_affected())
    }
}

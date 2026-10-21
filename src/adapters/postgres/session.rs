//! PostgreSQL [`SessionRepository`] adapter.

use super::{Db, now, ts};
use crate::domain::{Id, Session, Timestamp};
use crate::error::Result;
use crate::ports::SessionRepository;
use async_trait::async_trait;
use sqlx::FromRow;

#[derive(Clone)]
pub struct PgSessionRepository {
    db: Db,
}

impl PgSessionRepository {
    pub fn new(db: Db) -> Self {
        PgSessionRepository { db }
    }
}

/// Raw row mirroring the `sessions` table.
#[derive(FromRow)]
struct SessionRow {
    id: String,
    user_id: Id,
    created_at: Timestamp,
    expires_at: Timestamp,
}

impl From<SessionRow> for Session {
    fn from(row: SessionRow) -> Self {
        Session {
            id: row.id,
            user_id: row.user_id,
            created_at: row.created_at,
            expires_at: row.expires_at,
        }
    }
}

#[async_trait]
impl SessionRepository for PgSessionRepository {
    async fn create(&self, id: String, user_id: Id, expires_at: Timestamp) -> Result<Session> {
        let now = now();
        let expires_at = ts(expires_at);
        sqlx::query(
            "INSERT INTO sessions (id, user_id, created_at, expires_at) VALUES ($1, $2, $3, $4)",
        )
        .bind(&id)
        .bind(user_id)
        .bind(now)
        .bind(expires_at)
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
        // Expired sessions are treated as absent.
        let row = sqlx::query_as::<_, SessionRow>(
            "SELECT id, user_id, created_at, expires_at FROM sessions \
             WHERE id = $1 AND expires_at > $2",
        )
        .bind(id)
        .bind(now())
        .fetch_optional(&self.db)
        .await?;
        Ok(row.map(Session::from))
    }

    async fn delete(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(id)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    async fn delete_for_user(&self, user_id: Id) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(user_id)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    async fn delete_expired(&self, now: Timestamp) -> Result<u64> {
        let result = sqlx::query("DELETE FROM sessions WHERE expires_at <= $1")
            .bind(now)
            .execute(&self.db)
            .await?;
        Ok(result.rows_affected())
    }
}

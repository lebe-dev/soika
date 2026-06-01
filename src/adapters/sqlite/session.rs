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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::sqlite::tests::{insert_user, test_pool};
    use chrono::Duration;

    #[tokio::test]
    async fn create_find_delete() {
        let pool = test_pool().await;
        let user_id = insert_user(&pool, "s@example.com").await;
        let repo = SqliteSessionRepository::new(pool.clone());

        let expires = chrono::Utc::now() + Duration::hours(1);
        let created = repo
            .create("tok-1".to_string(), user_id, expires)
            .await
            .unwrap();
        assert_eq!(created.user_id, user_id);

        let found = repo.find("tok-1").await.unwrap().unwrap();
        assert_eq!(found.id, "tok-1");

        repo.delete("tok-1").await.unwrap();
        assert!(repo.find("tok-1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn expired_session_is_not_returned_and_can_be_purged() {
        let pool = test_pool().await;
        let user_id = insert_user(&pool, "e@example.com").await;
        let repo = SqliteSessionRepository::new(pool.clone());

        let past = chrono::Utc::now() - Duration::hours(1);
        repo.create("old".to_string(), user_id, past).await.unwrap();

        // find skips expired sessions.
        assert!(repo.find("old").await.unwrap().is_none());

        let purged = repo.delete_expired(chrono::Utc::now()).await.unwrap();
        assert_eq!(purged, 1);
    }

    #[tokio::test]
    async fn delete_for_user_removes_all() {
        let pool = test_pool().await;
        let user_id = insert_user(&pool, "d@example.com").await;
        let repo = SqliteSessionRepository::new(pool.clone());

        let expires = chrono::Utc::now() + Duration::hours(1);
        repo.create("a".to_string(), user_id, expires)
            .await
            .unwrap();
        repo.create("b".to_string(), user_id, expires)
            .await
            .unwrap();

        repo.delete_for_user(user_id).await.unwrap();
        assert!(repo.find("a").await.unwrap().is_none());
        assert!(repo.find("b").await.unwrap().is_none());
    }
}

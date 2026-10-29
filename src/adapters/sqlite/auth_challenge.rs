//! Database-backed one-use ceremony state.
use super::Db;
use super::write::retry_write;
use crate::{Result, config::WritePolicy, domain::Timestamp, ports::AuthChallengeRepository};
use async_trait::async_trait;

#[derive(Clone)]
pub struct SqliteAuthChallengeRepository {
    db: Db,
}
impl SqliteAuthChallengeRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl AuthChallengeRepository for SqliteAuthChallengeRepository {
    async fn create(
        &self,
        id: &str,
        purpose: &str,
        state: &str,
        expires_at: Timestamp,
    ) -> Result<()> {
        retry_write(
            &WritePolicy::default(),
            "store authentication challenge",
            || async {
                let mut tx = super::write::begin_write(&self.db).await?;
                sqlx::query("DELETE FROM auth_challenges WHERE id IN (SELECT id FROM auth_challenges WHERE expires_at <= ? LIMIT ?)")
                    .bind(super::ts_to_db(chrono::Utc::now()))
                    .bind(WritePolicy::default().delete_batch)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query(
                    "INSERT INTO auth_challenges (id, purpose, state, expires_at) \
                 VALUES (?, ?, ?, ?)",
                )
                .bind(id)
                .bind(purpose)
                .bind(state)
                .bind(super::ts_to_db(expires_at))
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
                Ok(())
            },
        )
        .await
    }
    async fn take(&self, id: &str, purpose: &str, now: Timestamp) -> Result<Option<String>> {
        retry_write(
            &WritePolicy::default(),
            "consume authentication challenge",
            || async {
                Ok(sqlx::query_scalar(
                    "DELETE FROM auth_challenges \
                 WHERE id = ? AND purpose = ? AND expires_at > ? RETURNING state",
                )
                .bind(id)
                .bind(purpose)
                .bind(super::ts_to_db(now))
                .fetch_optional(&self.db)
                .await?)
            },
        )
        .await
    }
}

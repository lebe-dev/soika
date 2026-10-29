//! Database-backed one-use ceremony state.
use super::Db;
use crate::adapters::retry::retry_write;
use crate::{Result, config::WritePolicy, domain::Timestamp, ports::AuthChallengeRepository};
use async_trait::async_trait;

#[derive(Clone)]
pub struct PgAuthChallengeRepository {
    db: Db,
}
impl PgAuthChallengeRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl AuthChallengeRepository for PgAuthChallengeRepository {
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
                let mut tx = self.db.begin().await?;
                sqlx::query("DELETE FROM auth_challenges WHERE id IN (SELECT id FROM auth_challenges WHERE expires_at <= $1 LIMIT $2)")
                    .bind(super::ts(chrono::Utc::now()))
                    .bind(WritePolicy::default().delete_batch)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query(
                    "INSERT INTO auth_challenges (id, purpose, state, expires_at) \
                 VALUES ($1, $2, $3, $4)",
                )
                .bind(id)
                .bind(purpose)
                .bind(state)
                .bind(super::ts(expires_at))
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
                 WHERE id = $1 AND purpose = $2 AND expires_at > $3 RETURNING state",
                )
                .bind(id)
                .bind(purpose)
                .bind(super::ts(now))
                .fetch_optional(&self.db)
                .await?)
            },
        )
        .await
    }
}

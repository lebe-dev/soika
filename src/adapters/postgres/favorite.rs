//! PostgreSQL [`FavoriteRepository`] adapter.

use super::{Db, now};
use crate::domain::Id;
use crate::error::Result;
use crate::ports::FavoriteRepository;
use async_trait::async_trait;

#[derive(Clone)]
pub struct PgFavoriteRepository {
    db: Db,
}

impl PgFavoriteRepository {
    pub fn new(db: Db) -> Self {
        PgFavoriteRepository { db }
    }
}

#[async_trait]
impl FavoriteRepository for PgFavoriteRepository {
    async fn add(&self, user_id: Id, project_id: Id) -> Result<()> {
        // Idempotent: favoriting twice is a no-op.
        sqlx::query(
            "INSERT INTO project_favorites (user_id, project_id, created_at) \
             VALUES ($1, $2, $3) ON CONFLICT (user_id, project_id) DO NOTHING",
        )
        .bind(user_id)
        .bind(project_id)
        .bind(now())
        .execute(&self.db)
        .await?;
        Ok(())
    }

    async fn remove(&self, user_id: Id, project_id: Id) -> Result<()> {
        sqlx::query("DELETE FROM project_favorites WHERE user_id = $1 AND project_id = $2")
            .bind(user_id)
            .bind(project_id)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    async fn list_for_user(&self, user_id: Id) -> Result<Vec<Id>> {
        let ids = sqlx::query_scalar("SELECT project_id FROM project_favorites WHERE user_id = $1")
            .bind(user_id)
            .fetch_all(&self.db)
            .await?;
        Ok(ids)
    }
}

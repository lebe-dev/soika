//! SQLite [`FavoriteRepository`] adapter.

use super::Db;
use crate::domain::Id;
use crate::error::Result;
use crate::ports::FavoriteRepository;
use async_trait::async_trait;
use sqlx::Row;

#[derive(Clone)]
pub struct SqliteFavoriteRepository {
    db: Db,
}

impl SqliteFavoriteRepository {
    pub fn new(db: Db) -> Self {
        SqliteFavoriteRepository { db }
    }
}

#[async_trait]
impl FavoriteRepository for SqliteFavoriteRepository {
    async fn add(&self, user_id: Id, project_id: Id) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT OR IGNORE INTO project_favorites (user_id, project_id, created_at) \
             VALUES (?, ?, ?)",
        )
        .bind(user_id.to_string())
        .bind(project_id.to_string())
        .bind(now)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    async fn remove(&self, user_id: Id, project_id: Id) -> Result<()> {
        sqlx::query("DELETE FROM project_favorites WHERE user_id = ? AND project_id = ?")
            .bind(user_id.to_string())
            .bind(project_id.to_string())
            .execute(&self.db)
            .await?;
        Ok(())
    }

    async fn list_for_user(&self, user_id: Id) -> Result<Vec<Id>> {
        let rows = sqlx::query("SELECT project_id FROM project_favorites WHERE user_id = ?")
            .bind(user_id.to_string())
            .fetch_all(&self.db)
            .await?;

        rows.iter()
            .map(|row| {
                let s: String = row.try_get("project_id")?;
                super::id_from_db(&s)
            })
            .collect()
    }
}

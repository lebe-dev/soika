//! PostgreSQL [`SettingsRepository`] adapter.
//!
//! Service settings live in a single-row table (`id = 1`), seeded by the
//! initial migration, so reads never miss and updates always target row 1.

use super::{Db, now};
use crate::adapters::retry::retry_write;
use crate::config::WritePolicy;
use crate::domain::{Id, ServiceSettings, Timestamp};
use crate::error::{Error, Result};
use crate::ports::SettingsRepository;
use async_trait::async_trait;
use sqlx::FromRow;

#[derive(Clone)]
pub struct PgSettingsRepository {
    db: Db,
}

impl PgSettingsRepository {
    pub fn new(db: Db) -> Self {
        PgSettingsRepository { db }
    }
}

#[derive(FromRow)]
struct SettingsRow {
    allow_signup: bool,
    org_name: String,
    updated_at: Timestamp,
}

impl From<SettingsRow> for ServiceSettings {
    fn from(row: SettingsRow) -> Self {
        ServiceSettings {
            allow_signup: row.allow_signup,
            org_name: row.org_name,
            updated_at: row.updated_at,
        }
    }
}

const SELECT_SETTINGS: &str = "SELECT allow_signup, org_name, updated_at FROM service_settings";

#[async_trait]
impl SettingsRepository for PgSettingsRepository {
    async fn default_team_ids(&self) -> Result<Vec<Id>> {
        Ok(
            sqlx::query_scalar("SELECT team_id FROM default_teams ORDER BY team_id")
                .fetch_all(&self.db)
                .await?,
        )
    }

    async fn set_default_team_ids(&self, mut team_ids: Vec<Id>) -> Result<()> {
        team_ids.sort_unstable();
        team_ids.dedup();
        retry_write(&WritePolicy::default(), "set default teams", || async {
            let mut tx = self.db.begin().await?;
            // Serialize replacements so concurrent saves cannot merge their lists.
            sqlx::query("SELECT id FROM service_settings WHERE id = 1 FOR UPDATE")
                .execute(&mut *tx)
                .await?;
            // Validate before replacing anything; FK references also protect deletion races.
            for id in &team_ids {
                let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM teams WHERE id = $1")
                    .bind(*id)
                    .fetch_one(&mut *tx)
                    .await?;
                if exists == 0 {
                    return Err(Error::validation(format!("unknown default team: {id}")));
                }
            }
            sqlx::query("DELETE FROM default_teams")
                .execute(&mut *tx)
                .await?;
            for id in &team_ids {
                sqlx::query("INSERT INTO default_teams (team_id) VALUES ($1)")
                    .bind(*id)
                    .execute(&mut *tx)
                    .await?;
            }
            sqlx::query("UPDATE service_settings SET updated_at = $1 WHERE id = 1")
                .bind(now())
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(())
        })
        .await
    }

    async fn get(&self) -> Result<ServiceSettings> {
        let row = sqlx::query_as::<_, SettingsRow>(sqlx::AssertSqlSafe(format!(
            "{SELECT_SETTINGS} WHERE id = 1"
        )))
        .fetch_one(&self.db)
        .await?;
        Ok(row.into())
    }

    async fn set_allow_signup(&self, allow: bool) -> Result<ServiceSettings> {
        let row = sqlx::query_as::<_, SettingsRow>(
            "UPDATE service_settings SET allow_signup = $1, updated_at = $2 WHERE id = 1 \
             RETURNING allow_signup, org_name, updated_at",
        )
        .bind(allow)
        .bind(now())
        .fetch_one(&self.db)
        .await?;
        Ok(row.into())
    }

    async fn set_org_name(&self, org_name: String) -> Result<ServiceSettings> {
        let row = sqlx::query_as::<_, SettingsRow>(
            "UPDATE service_settings SET org_name = $1, updated_at = $2 WHERE id = 1 \
             RETURNING allow_signup, org_name, updated_at",
        )
        .bind(&org_name)
        .bind(now())
        .fetch_one(&self.db)
        .await?;
        Ok(row.into())
    }
}

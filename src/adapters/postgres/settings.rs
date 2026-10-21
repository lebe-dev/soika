//! PostgreSQL [`SettingsRepository`] adapter.
//!
//! Service settings live in a single-row table (`id = 1`), seeded by the
//! initial migration, so reads never miss and updates always target row 1.

use super::{Db, now};
use crate::domain::{ServiceSettings, Timestamp};
use crate::error::Result;
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

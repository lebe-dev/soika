//! SQLite [`SettingsRepository`] adapter.
//!
//! Service settings live in a single-row table (`id = 1`), seeded by the
//! 0001 migration, so reads never miss and updates always target row 1.

use super::{Db, bool_from_db, bool_to_db, ts_from_db, ts_to_db};
use crate::domain::ServiceSettings;
use crate::error::Result;
use crate::ports::SettingsRepository;
use async_trait::async_trait;
use sqlx::FromRow;

/// The singleton settings row id (MVP §14).
const SETTINGS_ID: i64 = 1;

#[derive(Clone)]
pub struct SqliteSettingsRepository {
    db: Db,
}

impl SqliteSettingsRepository {
    pub fn new(db: Db) -> Self {
        SqliteSettingsRepository { db }
    }
}

#[derive(FromRow)]
struct SettingsRow {
    allow_signup: i64,
    org_name: String,
    updated_at: String,
}

impl SettingsRow {
    fn into_domain(self) -> Result<ServiceSettings> {
        Ok(ServiceSettings {
            allow_signup: bool_from_db(self.allow_signup),
            org_name: self.org_name,
            updated_at: ts_from_db(&self.updated_at)?,
        })
    }
}

impl SqliteSettingsRepository {
    async fn read(&self) -> Result<ServiceSettings> {
        let row = sqlx::query_as::<_, SettingsRow>(
            "SELECT allow_signup, org_name, updated_at FROM service_settings WHERE id = ?",
        )
        .bind(SETTINGS_ID)
        .fetch_one(&self.db)
        .await?;
        row.into_domain()
    }
}

#[async_trait]
impl SettingsRepository for SqliteSettingsRepository {
    async fn get(&self) -> Result<ServiceSettings> {
        self.read().await
    }

    async fn set_allow_signup(&self, allow: bool) -> Result<ServiceSettings> {
        let now = chrono::Utc::now();
        sqlx::query("UPDATE service_settings SET allow_signup = ?, updated_at = ? WHERE id = ?")
            .bind(bool_to_db(allow))
            .bind(ts_to_db(now))
            .bind(SETTINGS_ID)
            .execute(&self.db)
            .await?;
        self.read().await
    }

    async fn set_org_name(&self, org_name: String) -> Result<ServiceSettings> {
        let now = chrono::Utc::now();
        sqlx::query("UPDATE service_settings SET org_name = ?, updated_at = ? WHERE id = ?")
            .bind(&org_name)
            .bind(ts_to_db(now))
            .bind(SETTINGS_ID)
            .execute(&self.db)
            .await?;
        self.read().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::sqlite::tests::test_pool;

    #[tokio::test]
    async fn defaults_from_migration() {
        let repo = SqliteSettingsRepository::new(test_pool().await);
        let settings = repo.get().await.unwrap();
        assert!(!settings.allow_signup);
        assert_eq!(settings.org_name, "soika");
    }

    #[tokio::test]
    async fn toggle_allow_signup() {
        let repo = SqliteSettingsRepository::new(test_pool().await);
        let updated = repo.set_allow_signup(true).await.unwrap();
        assert!(updated.allow_signup);
        // org_name preserved.
        assert_eq!(updated.org_name, "soika");

        let read_back = repo.get().await.unwrap();
        assert!(read_back.allow_signup);
    }

    #[tokio::test]
    async fn set_org_name_persists() {
        let repo = SqliteSettingsRepository::new(test_pool().await);
        let updated = repo.set_org_name("Acme".to_string()).await.unwrap();
        assert_eq!(updated.org_name, "Acme");
        // allow_signup default preserved.
        assert!(!updated.allow_signup);

        assert_eq!(repo.get().await.unwrap().org_name, "Acme");
    }
}

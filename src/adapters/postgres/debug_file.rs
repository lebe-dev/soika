//! Persistent ProGuard mappings and temporary upload chunks.
use super::Db;
use crate::{Result, config::WritePolicy, domain::Id, ports::DebugFileRepository};
use async_trait::async_trait;
pub struct PgDebugFileRepository {
    db: Db,
}
impl PgDebugFileRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}
fn table(kind: &str) -> Result<&'static str> {
    match kind {
        "mapping" => Ok("proguard_mappings"),
        "chunk" => Ok("debug_upload_chunks"),
        _ => Err(crate::Error::internal("unknown debug artifact kind")),
    }
}
#[async_trait]
impl DebugFileRepository for PgDebugFileRepository {
    async fn put(&self, owner: Id, kind: &str, key: &str, data: &str) -> Result<()> {
        let table = table(kind)?;
        let now = chrono::Utc::now().timestamp();
        let expires = if kind == "chunk" {
            now + 86400
        } else {
            i64::MAX
        };
        let query = format!(
            "INSERT INTO {table} (owner_id, key, data, expires_at) VALUES ($1, $2, $3, $4) ON CONFLICT (owner_id, key) DO UPDATE SET data = excluded.data, expires_at = excluded.expires_at"
        );
        crate::adapters::retry::retry_write(&WritePolicy::default(), "store debug file", || async {
 let mut tx = self.db.begin().await?;
 sqlx::query("DELETE FROM debug_upload_chunks WHERE (owner_id, key) IN (SELECT owner_id, key FROM debug_upload_chunks WHERE expires_at <= $1 LIMIT 100)").bind(now).execute(&mut *tx).await?;
 sqlx::query(sqlx::AssertSqlSafe(query.as_str())).bind(owner).bind(key).bind(data).bind(expires).execute(&mut *tx).await?;
 tx.commit().await?;
 Ok(())
 }).await
    }
    async fn get(&self, owner: Id, kind: &str, key: &str) -> Result<Option<String>> {
        let table = table(kind)?;
        let query = format!(
            "SELECT data FROM {table} WHERE owner_id = $1 AND key = $2 AND expires_at > $3"
        );
        Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(query.as_str()))
            .bind(owner)
            .bind(key)
            .bind(chrono::Utc::now().timestamp())
            .fetch_optional(&self.db)
            .await?)
    }
}

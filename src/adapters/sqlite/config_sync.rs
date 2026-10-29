//! Sqlite representation and locking for the shared config-sync adapter.
use crate::adapters::config_sync::{ConfigDatabase, SqlConfigSync};
use crate::{Result, domain::Id};
use std::borrow::Cow;

pub type SqliteConfigSync = SqlConfigSync<sqlx::Sqlite>;

impl ConfigDatabase for sqlx::Sqlite {
    type Id = String;
    type Time = String;
    fn id(id: Id) -> Self::Id {
        id.to_string()
    }
    fn domain_id(id: Self::Id) -> Result<Id> {
        super::id_from_db(&id)
    }
    fn now() -> Self::Time {
        chrono::Utc::now().to_rfc3339()
    }
    fn sql(sql: &'static str) -> sqlx::AssertSqlSafe<Cow<'static, str>> {
        sqlx::AssertSqlSafe(Cow::Borrowed(sql))
    }
    fn rows_affected(result: Self::QueryResult) -> u64 {
        result.rows_affected()
    }
    async fn begin(db: &super::Db) -> Result<sqlx::Transaction<'_, Self>> {
        super::begin_write(db).await
    }
}

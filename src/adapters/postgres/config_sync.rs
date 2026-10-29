//! Postgres representation and locking for the shared config-sync adapter.
use crate::adapters::config_sync::{ConfigDatabase, SqlConfigSync};
use crate::{Result, domain::Id};
use std::borrow::Cow;

pub type PgConfigSync = SqlConfigSync<sqlx::Postgres>;

impl ConfigDatabase for sqlx::Postgres {
    type Id = Id;
    type Time = crate::domain::Timestamp;
    fn id(id: Id) -> Self::Id {
        id
    }
    fn domain_id(id: Self::Id) -> Result<Id> {
        Ok(id)
    }
    fn now() -> Self::Time {
        super::now()
    }
    fn sql(sql: &'static str) -> sqlx::AssertSqlSafe<Cow<'static, str>> {
        // Only fixed SQL literals enter here; all data stays in bind parameters.
        let mut index = 0;
        let sql = sql.chars().fold(String::new(), |mut out, ch| {
            if ch == '?' {
                index += 1;
                out.push('$');
                out.push_str(&index.to_string());
            } else {
                out.push(ch);
            }
            out
        });
        sqlx::AssertSqlSafe(Cow::Owned(sql))
    }
    fn rows_affected(result: Self::QueryResult) -> u64 {
        result.rows_affected()
    }
    async fn begin(db: &super::Db) -> Result<sqlx::Transaction<'_, Self>> {
        let mut tx = db.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(9182609)")
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }
}

//! PostgreSQL [`PasskeyRepository`] adapter.

use super::{Db, conflict_or_db, now, ts};
use crate::adapters::retry::retry_write;
use crate::config::WritePolicy;
use crate::domain::{Id, Passkey, Timestamp};
use crate::error::{Error, Result};
use crate::ports::{NewPasskey, PasskeyRepository};
use async_trait::async_trait;
use sqlx::{AssertSqlSafe, FromRow};

#[derive(Clone)]
pub struct PgPasskeyRepository {
    db: Db,
    write: WritePolicy,
}

impl PgPasskeyRepository {
    pub fn new(db: Db) -> Self {
        PgPasskeyRepository {
            db,
            write: WritePolicy::default(),
        }
    }
}

/// Raw row mirroring the `passkeys` table.
#[derive(FromRow)]
struct PasskeyRow {
    id: Id,
    user_id: Id,
    credential_id: String,
    name: String,
    credential: String,
    created_at: Timestamp,
    last_used_at: Option<Timestamp>,
}

impl From<PasskeyRow> for Passkey {
    fn from(row: PasskeyRow) -> Self {
        Passkey {
            id: row.id,
            user_id: row.user_id,
            credential_id: row.credential_id,
            name: row.name,
            credential: row.credential,
            created_at: row.created_at,
            last_used_at: row.last_used_at,
        }
    }
}

/// Column list shared by every read query.
const COLUMNS: &str = "id, user_id, credential_id, name, credential, created_at, last_used_at";

#[async_trait]
impl PasskeyRepository for PgPasskeyRepository {
    async fn create(&self, new: NewPasskey) -> Result<Passkey> {
        let id = Id::new_v4();
        let now = now();

        retry_write(&self.write, "insert passkey", || async {
            sqlx::query(
                "INSERT INTO passkeys (id, user_id, credential_id, name, credential, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(id)
            .bind(new.user_id)
            .bind(&new.credential_id)
            .bind(&new.name)
            .bind(&new.credential)
            .bind(now)
            .execute(&self.db)
            .await
            .map_err(|e| conflict_or_db(e, "passkey is already registered"))?;
            Ok(())
        })
        .await?;

        Ok(Passkey {
            id,
            user_id: new.user_id,
            credential_id: new.credential_id,
            name: new.name,
            credential: new.credential,
            created_at: now,
            last_used_at: None,
        })
    }

    async fn list_for_user(&self, user_id: Id) -> Result<Vec<Passkey>> {
        let rows = sqlx::query_as::<_, PasskeyRow>(AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM passkeys WHERE user_id = $1 ORDER BY created_at DESC"
        )))
        .bind(user_id)
        .fetch_all(&self.db)
        .await?;
        Ok(rows.into_iter().map(Passkey::from).collect())
    }

    async fn find_by_credential_id(&self, credential_id: &str) -> Result<Option<Passkey>> {
        let row = sqlx::query_as::<_, PasskeyRow>(AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM passkeys WHERE credential_id = $1"
        )))
        .bind(credential_id)
        .fetch_optional(&self.db)
        .await?;
        Ok(row.map(Passkey::from))
    }

    async fn find_for_user(&self, user_id: Id, id: Id) -> Result<Option<Passkey>> {
        let row = sqlx::query_as::<_, PasskeyRow>(AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM passkeys WHERE id = $1 AND user_id = $2"
        )))
        .bind(id)
        .bind(user_id)
        .fetch_optional(&self.db)
        .await?;
        Ok(row.map(Passkey::from))
    }

    async fn rename(&self, user_id: Id, id: Id, name: &str) -> Result<Passkey> {
        let row = retry_write(&self.write, "rename passkey", || async {
            let row = sqlx::query_as::<_, PasskeyRow>(AssertSqlSafe(format!(
                "UPDATE passkeys SET name = $1 WHERE id = $2 AND user_id = $3 \
                 RETURNING {COLUMNS}"
            )))
            .bind(name)
            .bind(id)
            .bind(user_id)
            .fetch_optional(&self.db)
            .await?;
            Ok(row)
        })
        .await?;

        row.map(Passkey::from)
            .ok_or_else(|| Error::not_found("passkey"))
    }

    async fn touch(&self, id: Id, credential: &str, used_at: Timestamp) -> Result<()> {
        let used_at = ts(used_at);
        retry_write(&self.write, "update passkey after assertion", || async {
            sqlx::query("UPDATE passkeys SET credential = $1, last_used_at = $2 WHERE id = $3")
                .bind(credential)
                .bind(used_at)
                .bind(id)
                .execute(&self.db)
                .await?;
            Ok(())
        })
        .await
    }

    async fn delete(&self, user_id: Id, id: Id) -> Result<()> {
        let affected = retry_write(&self.write, "delete passkey", || async {
            let result = sqlx::query("DELETE FROM passkeys WHERE id = $1 AND user_id = $2")
                .bind(id)
                .bind(user_id)
                .execute(&self.db)
                .await?;
            Ok(result.rows_affected())
        })
        .await?;

        if affected == 0 {
            return Err(Error::not_found("passkey"));
        }
        Ok(())
    }

    async fn count_for_user(&self, user_id: Id) -> Result<i64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM passkeys WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&self.db)
            .await?;
        Ok(count)
    }
}

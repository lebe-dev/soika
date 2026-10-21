//! SQLite [`PasskeyRepository`] adapter.

use super::{Db, conflict_or_db, id_from_db, id_to_db, retry_write, ts_from_db, ts_to_db};
use crate::domain::{Id, Passkey, Timestamp};
use crate::error::{Error, Result};
use crate::ports::{NewPasskey, PasskeyRepository};
use async_trait::async_trait;
use sqlx::AssertSqlSafe;
use sqlx::FromRow;

use crate::config::WritePolicy;

#[derive(Clone)]
pub struct SqlitePasskeyRepository {
    db: Db,
    write: WritePolicy,
}

impl SqlitePasskeyRepository {
    pub fn new(db: Db) -> Self {
        SqlitePasskeyRepository {
            db,
            write: WritePolicy::default(),
        }
    }
}

/// Raw row mirroring the `passkeys` table.
#[derive(FromRow)]
struct PasskeyRow {
    id: String,
    user_id: String,
    credential_id: String,
    name: String,
    credential: String,
    created_at: String,
    last_used_at: Option<String>,
}

/// Column list shared by every read query.
const COLUMNS: &str = "id, user_id, credential_id, name, credential, created_at, last_used_at";

impl PasskeyRow {
    fn into_domain(self) -> Result<Passkey> {
        let last_used_at = match self.last_used_at.as_deref() {
            Some(raw) => Some(ts_from_db(raw)?),
            None => None,
        };
        Ok(Passkey {
            id: id_from_db(&self.id)?,
            user_id: id_from_db(&self.user_id)?,
            credential_id: self.credential_id,
            name: self.name,
            credential: self.credential,
            created_at: ts_from_db(&self.created_at)?,
            last_used_at,
        })
    }
}

#[async_trait]
impl PasskeyRepository for SqlitePasskeyRepository {
    async fn create(&self, new: NewPasskey) -> Result<Passkey> {
        let id = Id::new_v4();
        let now: Timestamp = chrono::Utc::now();
        let created_at = ts_to_db(now);

        retry_write(&self.write, "insert passkey", || {
            let new = new.clone();
            let created_at = created_at.clone();
            async move {
                sqlx::query(
                    "INSERT INTO passkeys (id, user_id, credential_id, name, credential, created_at) \
                     VALUES (?, ?, ?, ?, ?, ?)",
                )
                .bind(id_to_db(id))
                .bind(id_to_db(new.user_id))
                .bind(&new.credential_id)
                .bind(&new.name)
                .bind(&new.credential)
                .bind(&created_at)
                .execute(&self.db)
                .await
                .map_err(|e| conflict_or_db(e, "passkey is already registered"))?;
                Ok(())
            }
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
            "SELECT {COLUMNS} FROM passkeys WHERE user_id = ? ORDER BY created_at DESC"
        )))
        .bind(id_to_db(user_id))
        .fetch_all(&self.db)
        .await?;
        rows.into_iter().map(PasskeyRow::into_domain).collect()
    }

    async fn find_by_credential_id(&self, credential_id: &str) -> Result<Option<Passkey>> {
        let row = sqlx::query_as::<_, PasskeyRow>(AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM passkeys WHERE credential_id = ?"
        )))
        .bind(credential_id)
        .fetch_optional(&self.db)
        .await?;
        row.map(PasskeyRow::into_domain).transpose()
    }

    async fn find_for_user(&self, user_id: Id, id: Id) -> Result<Option<Passkey>> {
        let row = sqlx::query_as::<_, PasskeyRow>(AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM passkeys WHERE id = ? AND user_id = ?"
        )))
        .bind(id_to_db(id))
        .bind(id_to_db(user_id))
        .fetch_optional(&self.db)
        .await?;
        row.map(PasskeyRow::into_domain).transpose()
    }

    async fn rename(&self, user_id: Id, id: Id, name: &str) -> Result<Passkey> {
        let affected = retry_write(&self.write, "rename passkey", || async {
            let result = sqlx::query("UPDATE passkeys SET name = ? WHERE id = ? AND user_id = ?")
                .bind(name)
                .bind(id_to_db(id))
                .bind(id_to_db(user_id))
                .execute(&self.db)
                .await?;
            Ok(result.rows_affected())
        })
        .await?;

        if affected == 0 {
            return Err(Error::not_found("passkey"));
        }
        self.find_for_user(user_id, id)
            .await?
            .ok_or_else(|| Error::not_found("passkey"))
    }

    async fn touch(&self, id: Id, credential: &str, used_at: Timestamp) -> Result<()> {
        let used_at = ts_to_db(used_at);
        retry_write(&self.write, "update passkey after assertion", || {
            let used_at = used_at.clone();
            async move {
                sqlx::query("UPDATE passkeys SET credential = ?, last_used_at = ? WHERE id = ?")
                    .bind(credential)
                    .bind(&used_at)
                    .bind(id_to_db(id))
                    .execute(&self.db)
                    .await?;
                Ok(())
            }
        })
        .await
    }

    async fn delete(&self, user_id: Id, id: Id) -> Result<()> {
        let affected = retry_write(&self.write, "delete passkey", || async {
            let result = sqlx::query("DELETE FROM passkeys WHERE id = ? AND user_id = ?")
                .bind(id_to_db(id))
                .bind(id_to_db(user_id))
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
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM passkeys WHERE user_id = ?")
            .bind(id_to_db(user_id))
            .fetch_one(&self.db)
            .await?;
        Ok(count)
    }
}

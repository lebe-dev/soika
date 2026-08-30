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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::sqlite::tests::{insert_user, test_pool};

    fn new_key(user_id: Id, credential_id: &str) -> NewPasskey {
        NewPasskey {
            user_id,
            credential_id: credential_id.to_string(),
            name: "MacBook".to_string(),
            credential: "{\"cred\":true}".to_string(),
        }
    }

    #[tokio::test]
    async fn create_list_find_delete() {
        let pool = test_pool().await;
        let user_id = insert_user(&pool, "p@example.com").await;
        let repo = SqlitePasskeyRepository::new(pool.clone());

        let created = repo.create(new_key(user_id, "cred-1")).await.unwrap();
        assert_eq!(created.user_id, user_id);
        assert!(created.last_used_at.is_none());

        let listed = repo.list_for_user(user_id).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(repo.count_for_user(user_id).await.unwrap(), 1);

        let found = repo.find_by_credential_id("cred-1").await.unwrap().unwrap();
        assert_eq!(found.id, created.id);

        repo.delete(user_id, created.id).await.unwrap();
        assert!(repo.list_for_user(user_id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn duplicate_credential_id_is_a_conflict() {
        let pool = test_pool().await;
        let first = insert_user(&pool, "a@example.com").await;
        let second = insert_user(&pool, "b@example.com").await;
        let repo = SqlitePasskeyRepository::new(pool.clone());

        repo.create(new_key(first, "shared")).await.unwrap();
        let err = repo.create(new_key(second, "shared")).await.unwrap_err();
        assert!(matches!(err, Error::Conflict(_)));
    }

    #[tokio::test]
    async fn touch_persists_credential_and_last_used() {
        let pool = test_pool().await;
        let user_id = insert_user(&pool, "t@example.com").await;
        let repo = SqlitePasskeyRepository::new(pool.clone());
        let created = repo.create(new_key(user_id, "cred-2")).await.unwrap();

        let used_at = chrono::Utc::now();
        repo.touch(created.id, "{\"cred\":\"updated\"}", used_at)
            .await
            .unwrap();

        let found = repo
            .find_for_user(user_id, created.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.credential, "{\"cred\":\"updated\"}");
        assert!(found.last_used_at.is_some());
    }

    #[tokio::test]
    async fn rename_and_delete_are_scoped_to_the_owner() {
        let pool = test_pool().await;
        let owner = insert_user(&pool, "owner@example.com").await;
        let other = insert_user(&pool, "other@example.com").await;
        let repo = SqlitePasskeyRepository::new(pool.clone());
        let created = repo.create(new_key(owner, "cred-3")).await.unwrap();

        let renamed = repo.rename(owner, created.id, "iPhone").await.unwrap();
        assert_eq!(renamed.name, "iPhone");

        let err = repo.rename(other, created.id, "stolen").await.unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));

        let err = repo.delete(other, created.id).await.unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
        assert_eq!(repo.count_for_user(owner).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn deleting_the_user_cascades_to_passkeys() {
        let pool = test_pool().await;
        let user_id = insert_user(&pool, "cascade@example.com").await;
        let repo = SqlitePasskeyRepository::new(pool.clone());
        repo.create(new_key(user_id, "cred-4")).await.unwrap();

        sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(id_to_db(user_id))
            .execute(&pool)
            .await
            .unwrap();

        assert!(
            repo.find_by_credential_id("cred-4")
                .await
                .unwrap()
                .is_none()
        );
    }
}

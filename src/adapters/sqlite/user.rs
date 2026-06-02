//! SQLite [`UserRepository`] adapter.

use super::{
    Db, bool_from_db, bool_to_db, conflict_or_db, id_from_db, id_to_db, ts_from_db, ts_to_db,
};
use crate::domain::{AuthProvider, Id, Timestamp, User};
use crate::error::{Error, Result};
use crate::ports::{NewUser, UserRepository, UserUpdate};
use async_trait::async_trait;
use sqlx::FromRow;

#[derive(Clone)]
pub struct SqliteUserRepository {
    db: Db,
}

impl SqliteUserRepository {
    pub fn new(db: Db) -> Self {
        SqliteUserRepository { db }
    }
}

/// Raw row mirroring the `users` table; mapped to [`User`] via [`UserRow::into_domain`].
#[derive(FromRow)]
struct UserRow {
    id: String,
    email: String,
    display_name: String,
    password_hash: String,
    is_admin: i64,
    notifications_enabled: i64,
    auth_provider: String,
    created_at: String,
    updated_at: String,
}

impl UserRow {
    fn into_domain(self) -> Result<User> {
        Ok(User {
            id: id_from_db(&self.id)?,
            email: self.email,
            display_name: self.display_name,
            password_hash: self.password_hash,
            is_admin: bool_from_db(self.is_admin),
            notifications_enabled: bool_from_db(self.notifications_enabled),
            auth_provider: AuthProvider::from_db(&self.auth_provider),
            created_at: ts_from_db(&self.created_at)?,
            updated_at: ts_from_db(&self.updated_at)?,
        })
    }
}

const SELECT_USER: &str = "SELECT id, email, display_name, password_hash, is_admin, \
    notifications_enabled, auth_provider, created_at, updated_at FROM users";

#[async_trait]
impl UserRepository for SqliteUserRepository {
    async fn create(&self, new: NewUser) -> Result<User> {
        let id = Id::new_v4();
        let now: Timestamp = chrono::Utc::now();

        sqlx::query(
            "INSERT INTO users (id, email, display_name, password_hash, is_admin, \
             notifications_enabled, auth_provider, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, 1, ?, ?, ?)",
        )
        .bind(id_to_db(id))
        .bind(&new.email)
        .bind(&new.display_name)
        .bind(&new.password_hash)
        .bind(bool_to_db(new.is_admin))
        .bind(new.auth_provider.as_str())
        .bind(ts_to_db(now))
        .bind(ts_to_db(now))
        .execute(&self.db)
        .await
        .map_err(|e| conflict_or_db(e, format!("user email already exists: {}", new.email)))?;

        Ok(User {
            id,
            email: new.email,
            display_name: new.display_name,
            password_hash: new.password_hash,
            is_admin: new.is_admin,
            notifications_enabled: true,
            auth_provider: new.auth_provider,
            created_at: now,
            updated_at: now,
        })
    }

    async fn find_by_id(&self, id: Id) -> Result<Option<User>> {
        let row = sqlx::query_as::<_, UserRow>(&format!("{SELECT_USER} WHERE id = ?"))
            .bind(id_to_db(id))
            .fetch_optional(&self.db)
            .await?;
        row.map(UserRow::into_domain).transpose()
    }

    async fn find_by_email(&self, email: &str) -> Result<Option<User>> {
        let row = sqlx::query_as::<_, UserRow>(&format!("{SELECT_USER} WHERE email = ?"))
            .bind(email)
            .fetch_optional(&self.db)
            .await?;
        row.map(UserRow::into_domain).transpose()
    }

    async fn update(&self, id: Id, update: UserUpdate) -> Result<User> {
        // Apply only the provided fields; always bump updated_at.
        let now: Timestamp = chrono::Utc::now();

        let mut sql = String::from("UPDATE users SET updated_at = ?");
        if update.display_name.is_some() {
            sql.push_str(", display_name = ?");
        }
        if update.password_hash.is_some() {
            sql.push_str(", password_hash = ?");
        }
        if update.notifications_enabled.is_some() {
            sql.push_str(", notifications_enabled = ?");
        }
        sql.push_str(" WHERE id = ?");

        let mut query = sqlx::query(&sql).bind(ts_to_db(now));
        if let Some(name) = &update.display_name {
            query = query.bind(name.as_str());
        }
        if let Some(hash) = &update.password_hash {
            query = query.bind(hash.as_str());
        }
        if let Some(enabled) = update.notifications_enabled {
            query = query.bind(bool_to_db(enabled));
        }
        query = query.bind(id_to_db(id));

        let result = query.execute(&self.db).await?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found(format!("user not found: {id}")));
        }

        self.find_by_id(id)
            .await?
            .ok_or_else(|| Error::not_found(format!("user not found: {id}")))
    }

    async fn list(&self) -> Result<Vec<User>> {
        let rows = sqlx::query_as::<_, UserRow>(&format!("{SELECT_USER} ORDER BY created_at"))
            .fetch_all(&self.db)
            .await?;
        rows.into_iter().map(UserRow::into_domain).collect()
    }

    async fn delete(&self, id: Id) -> Result<()> {
        sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(id_to_db(id))
            .execute(&self.db)
            .await?;
        Ok(())
    }

    async fn count(&self) -> Result<i64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&self.db)
            .await?;
        Ok(count)
    }

    async fn count_admins(&self) -> Result<i64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE is_admin = 1")
            .fetch_one(&self.db)
            .await?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::sqlite::tests::test_pool;

    fn sample(email: &str) -> NewUser {
        NewUser {
            email: email.to_string(),
            display_name: "Test User".to_string(),
            password_hash: "$argon2id$v=19$m=4096,t=3,p=1$abc$def".to_string(),
            is_admin: false,
            auth_provider: AuthProvider::Local,
        }
    }

    fn oidc_sample(email: &str) -> NewUser {
        NewUser {
            email: email.to_string(),
            display_name: "OIDC User".to_string(),
            // OIDC accounts carry an empty-string password sentinel.
            password_hash: String::new(),
            is_admin: false,
            auth_provider: AuthProvider::Oidc,
        }
    }

    #[tokio::test]
    async fn create_and_find_roundtrip() {
        let repo = SqliteUserRepository::new(test_pool().await);
        let created = repo.create(sample("a@example.com")).await.unwrap();

        assert_eq!(created.email, "a@example.com");
        assert!(!created.is_admin);
        assert!(created.notifications_enabled);
        assert_eq!(created.auth_provider, AuthProvider::Local);

        let by_id = repo.find_by_id(created.id).await.unwrap().unwrap();
        assert_eq!(by_id.id, created.id);
        assert_eq!(by_id.password_hash, created.password_hash);
        assert_eq!(by_id.auth_provider, AuthProvider::Local);

        let by_email = repo.find_by_email("a@example.com").await.unwrap().unwrap();
        assert_eq!(by_email.id, created.id);
        assert_eq!(by_email.auth_provider, AuthProvider::Local);

        assert!(
            repo.find_by_email("missing@example.com")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn create_oidc_user_with_empty_hash_roundtrips() {
        let repo = SqliteUserRepository::new(test_pool().await);
        let created = repo.create(oidc_sample("sso@example.com")).await.unwrap();

        assert_eq!(created.auth_provider, AuthProvider::Oidc);
        assert_eq!(created.password_hash, "");

        let by_id = repo.find_by_id(created.id).await.unwrap().unwrap();
        assert_eq!(by_id.auth_provider, AuthProvider::Oidc);
        assert_eq!(by_id.password_hash, "");
    }

    #[tokio::test]
    async fn find_by_email_finds_local_and_oidc_users() {
        let repo = SqliteUserRepository::new(test_pool().await);
        repo.create(sample("local@example.com")).await.unwrap();
        repo.create(oidc_sample("oidc@example.com")).await.unwrap();

        let local = repo
            .find_by_email("local@example.com")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(local.auth_provider, AuthProvider::Local);

        let oidc = repo
            .find_by_email("oidc@example.com")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(oidc.auth_provider, AuthProvider::Oidc);
    }

    #[tokio::test]
    async fn duplicate_email_is_conflict() {
        let repo = SqliteUserRepository::new(test_pool().await);
        repo.create(sample("dup@example.com")).await.unwrap();
        let err = repo.create(sample("dup@example.com")).await.unwrap_err();
        assert!(matches!(err, Error::Conflict(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn partial_update_only_touches_given_fields() {
        let repo = SqliteUserRepository::new(test_pool().await);
        let created = repo.create(sample("u@example.com")).await.unwrap();

        let updated = repo
            .update(
                created.id,
                UserUpdate {
                    display_name: Some("Renamed".to_string()),
                    notifications_enabled: Some(false),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.display_name, "Renamed");
        assert!(!updated.notifications_enabled);
        // password_hash untouched
        assert_eq!(updated.password_hash, created.password_hash);
    }

    #[tokio::test]
    async fn update_missing_user_is_not_found() {
        let repo = SqliteUserRepository::new(test_pool().await);
        let err = repo
            .update(Id::new_v4(), UserUpdate::default())
            .await
            .unwrap_err();
        assert!(matches!(err, Error::NotFound(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn count_admins_counts_only_admins() {
        let repo = SqliteUserRepository::new(test_pool().await);
        assert_eq!(repo.count_admins().await.unwrap(), 0);

        // A non-admin account does not count toward initialization.
        repo.create(sample("member@example.com")).await.unwrap();
        assert_eq!(repo.count_admins().await.unwrap(), 0);

        let mut admin = sample("admin@example.com");
        admin.is_admin = true;
        repo.create(admin).await.unwrap();
        assert_eq!(repo.count_admins().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn list_count_and_delete() {
        let repo = SqliteUserRepository::new(test_pool().await);
        assert_eq!(repo.count().await.unwrap(), 0);

        let a = repo.create(sample("a@example.com")).await.unwrap();
        repo.create(sample("b@example.com")).await.unwrap();
        assert_eq!(repo.count().await.unwrap(), 2);
        assert_eq!(repo.list().await.unwrap().len(), 2);

        repo.delete(a.id).await.unwrap();
        assert_eq!(repo.count().await.unwrap(), 1);
        assert!(repo.find_by_id(a.id).await.unwrap().is_none());
        // Deleting a non-existent user is a no-op, not an error.
        repo.delete(Id::new_v4()).await.unwrap();
    }
}

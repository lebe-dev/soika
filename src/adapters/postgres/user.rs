//! PostgreSQL [`UserRepository`] adapter.

use super::{Db, conflict_or_db, now};
use crate::adapters::retry::retry_write;
use crate::config::WritePolicy;
use crate::domain::{AuthProvider, Id, InstanceRole, Timestamp, User, UserStatus};
use crate::error::{Error, Result};
use crate::ports::{NewUser, UserRepository, UserUpdate};
use async_trait::async_trait;
use sqlx::{FromRow, Postgres, QueryBuilder};

#[derive(Clone)]
pub struct PgUserRepository {
    db: Db,
}

impl PgUserRepository {
    pub fn new(db: Db) -> Self {
        PgUserRepository { db }
    }
}

/// Raw row mirroring the `users` table; mapped to [`User`] via [`UserRow::into_domain`].
/// Also embedded (flattened) by the team adapter's member join.
#[derive(FromRow)]
pub(super) struct UserRow {
    id: Id,
    email: String,
    display_name: String,
    password_hash: String,
    instance_role: String,
    notifications_enabled: bool,
    locale: Option<String>,
    auth_provider: String,
    status: String,
    created_at: Timestamp,
    updated_at: Timestamp,
}

impl UserRow {
    pub(super) fn into_domain(self) -> Result<User> {
        Ok(User {
            id: self.id,
            email: self.email,
            display_name: self.display_name,
            password_hash: self.password_hash,
            instance_role: InstanceRole::from_db(&self.instance_role),
            notifications_enabled: self.notifications_enabled,
            locale: self.locale.as_deref().map(str::parse).transpose()?,
            auth_provider: AuthProvider::from_db(&self.auth_provider),
            status: UserStatus::from_db(&self.status),
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

const SELECT_USER: &str = "SELECT id, email, display_name, password_hash, instance_role, \
    notifications_enabled, locale, auth_provider, status, created_at, updated_at FROM users";

#[async_trait]
impl UserRepository for PgUserRepository {
    async fn create(&self, new: NewUser) -> Result<User> {
        let id = Id::new_v4();
        let now = now();

        retry_write(
            &WritePolicy::default(),
            "create user with default teams",
            || async {
                let mut tx = self.db.begin().await?;
                sqlx::query(
                    "INSERT INTO users (id, email, display_name, password_hash, instance_role, \
                     notifications_enabled, auth_provider, status, created_at, updated_at) \
                     VALUES ($1, $2, $3, $4, $5, TRUE, $6, $7, $8, $8)",
                )
                .bind(id)
                .bind(&new.email)
                .bind(&new.display_name)
                .bind(&new.password_hash)
                .bind(new.instance_role.as_str())
                .bind(new.auth_provider.as_str())
                .bind(new.status.as_str())
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    conflict_or_db(e, format!("user email already exists: {}", new.email))
                })?;
                sqlx::query(
                    "INSERT INTO team_members (team_id, user_id, role, created_at) \
                     SELECT team_id, $1, 'contributor', $2 FROM default_teams",
                )
                .bind(id)
                .bind(now)
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
                Ok(())
            },
        )
        .await?;

        Ok(User {
            id,
            email: new.email,
            display_name: new.display_name,
            password_hash: new.password_hash,
            instance_role: new.instance_role,
            notifications_enabled: true,
            locale: None,
            auth_provider: new.auth_provider,
            status: new.status,
            created_at: now,
            updated_at: now,
        })
    }

    async fn find_by_id(&self, id: Id) -> Result<Option<User>> {
        let row = sqlx::query_as::<_, UserRow>(sqlx::AssertSqlSafe(format!(
            "{SELECT_USER} WHERE id = $1"
        )))
        .bind(id)
        .fetch_optional(&self.db)
        .await?;
        row.map(UserRow::into_domain).transpose()
    }

    async fn find_by_email(&self, email: &str) -> Result<Option<User>> {
        let row = sqlx::query_as::<_, UserRow>(sqlx::AssertSqlSafe(format!(
            "{SELECT_USER} WHERE email = $1"
        )))
        .bind(email)
        .fetch_optional(&self.db)
        .await?;
        row.map(UserRow::into_domain).transpose()
    }

    async fn update(&self, id: Id, update: UserUpdate) -> Result<User> {
        // Apply only the provided fields; always bump updated_at.
        let now = now();

        let result = retry_write(&WritePolicy::default(), "update user profile", || async {
            let mut query = QueryBuilder::<Postgres>::new("UPDATE users SET updated_at = ");
            query.push_bind(now);
            if let Some(name) = &update.display_name {
                query.push(", display_name = ").push_bind(name.as_str());
            }
            if let Some(hash) = &update.password_hash {
                query.push(", password_hash = ").push_bind(hash.as_str());
            }
            if let Some(enabled) = update.notifications_enabled {
                query.push(", notifications_enabled = ").push_bind(enabled);
            }
            if let Some(locale) = update.locale {
                query.push(", locale = ").push_bind(locale.as_str());
            } else if let Some(locale) = update.initial_locale {
                // Set only if no preference has been saved, atomically.
                query
                    .push(", locale = COALESCE(locale, ")
                    .push_bind(locale.as_str())
                    .push(")");
            }
            if let Some(status) = update.status {
                query.push(", status = ").push_bind(status.as_str());
            }
            query.push(" WHERE id = ").push_bind(id);

            Ok(query.build().execute(&self.db).await?)
        })
        .await?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found(format!("user not found: {id}")));
        }

        self.find_by_id(id)
            .await?
            .ok_or_else(|| Error::not_found(format!("user not found: {id}")))
    }

    async fn set_instance_role(&self, id: Id, role: InstanceRole) -> Result<User> {
        let result =
            sqlx::query("UPDATE users SET instance_role = $1, updated_at = $2 WHERE id = $3")
                .bind(role.as_str())
                .bind(now())
                .bind(id)
                .execute(&self.db)
                .await?;
        if result.rows_affected() == 0 {
            return Err(Error::not_found(format!("user not found: {id}")));
        }

        self.find_by_id(id)
            .await?
            .ok_or_else(|| Error::not_found(format!("user not found: {id}")))
    }

    async fn list(&self) -> Result<Vec<User>> {
        let rows = sqlx::query_as::<_, UserRow>(sqlx::AssertSqlSafe(format!(
            "{SELECT_USER} ORDER BY created_at, id"
        )))
        .fetch_all(&self.db)
        .await?;
        rows.into_iter().map(UserRow::into_domain).collect()
    }

    async fn delete(&self, id: Id) -> Result<()> {
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(id)
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

    async fn count_owners(&self) -> Result<i64> {
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE instance_role = 'owner'")
                .fetch_one(&self.db)
                .await?;
        Ok(count)
    }
}

//! SQLite [`UserRepository`] adapter.

use super::write::{begin_write, retry_write};
use super::{
    Db, bool_from_db, bool_to_db, conflict_or_db, id_from_db, id_to_db, ts_from_db, ts_to_db,
};
use crate::config::WritePolicy;
use crate::domain::{AuthProvider, Id, InstanceRole, Timestamp, User, UserStatus};
use crate::error::{Error, Result};
use crate::ports::{NewUser, UserRepository, UserUpdate};
use async_trait::async_trait;
use sqlx::FromRow;

#[derive(Clone)]
pub struct SqliteUserRepository {
    db: Db,
    assign_default_teams: bool,
}

impl SqliteUserRepository {
    pub fn new(db: Db) -> Self {
        SqliteUserRepository {
            db,
            assign_default_teams: true,
        }
    }
    pub fn with_default_teams(mut self, enabled: bool) -> Self {
        self.assign_default_teams = enabled;
        self
    }
}

/// Raw row mirroring the `users` table; mapped to [`User`] via [`UserRow::into_domain`].
#[derive(FromRow)]
struct UserRow {
    id: String,
    email: String,
    display_name: String,
    password_hash: String,
    instance_role: String,
    notifications_enabled: i64,
    locale: Option<String>,
    auth_provider: String,
    status: String,
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
            instance_role: InstanceRole::from_db(&self.instance_role),
            notifications_enabled: bool_from_db(self.notifications_enabled),
            locale: self.locale.as_deref().map(str::parse).transpose()?,
            auth_provider: AuthProvider::from_db(&self.auth_provider),
            status: UserStatus::from_db(&self.status),
            created_at: ts_from_db(&self.created_at)?,
            updated_at: ts_from_db(&self.updated_at)?,
        })
    }
}

const SELECT_USER: &str = "SELECT id, email, display_name, password_hash, instance_role, \
    notifications_enabled, locale, auth_provider, status, created_at, updated_at FROM users";

#[async_trait]
impl UserRepository for SqliteUserRepository {
    async fn create(&self, new: NewUser) -> Result<User> {
        let id = Id::new_v4();
        let now: Timestamp = chrono::Utc::now();

        retry_write(
            &WritePolicy::default(),
            "create user with default teams",
            || async {
                let mut tx = begin_write(&self.db).await?;
                sqlx::query(
                    "INSERT INTO users (id, email, display_name, password_hash, instance_role, \
                     notifications_enabled, auth_provider, status, created_at, updated_at) \
                     VALUES (?, ?, ?, ?, ?, 1, ?, ?, ?, ?)",
                )
                .bind(id_to_db(id))
                .bind(&new.email)
                .bind(&new.display_name)
                .bind(&new.password_hash)
                .bind(new.instance_role.as_str())
                .bind(new.auth_provider.as_str())
                .bind(new.status.as_str())
                .bind(ts_to_db(now))
                .bind(ts_to_db(now))
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    conflict_or_db(e, format!("user email already exists: {}", new.email))
                })?;
                if self.assign_default_teams {
                    sqlx::query(
                        "INSERT INTO team_members (team_id, user_id, role, created_at) \
                     SELECT team_id, ?, 'contributor', ? FROM default_teams",
                    )
                    .bind(id_to_db(id))
                    .bind(ts_to_db(now))
                    .execute(&mut *tx)
                    .await?;
                }
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
            "{SELECT_USER} WHERE id = ?"
        )))
        .bind(id_to_db(id))
        .fetch_optional(&self.db)
        .await?;
        row.map(UserRow::into_domain).transpose()
    }

    async fn find_by_email(&self, email: &str) -> Result<Option<User>> {
        let row = sqlx::query_as::<_, UserRow>(sqlx::AssertSqlSafe(format!(
            "{SELECT_USER} WHERE email = ?"
        )))
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
        if update.locale.is_some() {
            sql.push_str(", locale = ?");
        } else if update.initial_locale.is_some() {
            sql.push_str(", locale = COALESCE(locale, ?)");
        }
        if update.status.is_some() {
            sql.push_str(", status = ?");
        }
        sql.push_str(" WHERE id = ?");

        let result = retry_write(&WritePolicy::default(), "update user profile", || async {
            let mut query = sqlx::query(sqlx::AssertSqlSafe(&*sql)).bind(ts_to_db(now));
            if let Some(name) = &update.display_name {
                query = query.bind(name.as_str());
            }
            if let Some(hash) = &update.password_hash {
                query = query.bind(hash.as_str());
            }
            if let Some(enabled) = update.notifications_enabled {
                query = query.bind(bool_to_db(enabled));
            }
            if let Some(locale) = update.locale.or(update.initial_locale) {
                query = query.bind(locale.as_str());
            }
            if let Some(status) = update.status {
                query = query.bind(status.as_str());
            }
            query = query.bind(id_to_db(id));

            Ok(query.execute(&self.db).await?)
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
        let now: Timestamp = chrono::Utc::now();
        let result = sqlx::query("UPDATE users SET instance_role = ?, updated_at = ? WHERE id = ?")
            .bind(role.as_str())
            .bind(ts_to_db(now))
            .bind(id_to_db(id))
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
            "{SELECT_USER} ORDER BY created_at"
        )))
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

    async fn convert_to_sso(&self, id: Id) -> Result<User> {
        retry_write(
            &WritePolicy::default(),
            "convert account to SSO",
            || async {
                let mut tx = super::write::begin_write(&self.db).await?;
                // Lock the account before replacing its authentication material.
                let role: Option<String> =
                    sqlx::query_scalar("SELECT instance_role FROM users WHERE id = ?")
                        .bind(id_to_db(id))
                        .fetch_optional(&mut *tx)
                        .await?;
                match role.as_deref() {
                    None => return Err(Error::not_found("user not found")),
                    Some("owner") => {
                        return Err(Error::Forbidden(
                            "owner account cannot be converted to SSO-only".into(),
                        ));
                    }
                    _ => {}
                }
                sqlx::query(
                    "UPDATE users SET auth_provider = 'oidc', password_hash = '' WHERE id = ?",
                )
                .bind(id_to_db(id))
                .execute(&mut *tx)
                .await?;
                sqlx::query("DELETE FROM sessions WHERE user_id = ?")
                    .bind(id_to_db(id))
                    .execute(&mut *tx)
                    .await?;
                sqlx::query("DELETE FROM passkeys WHERE user_id = ?")
                    .bind(id_to_db(id))
                    .execute(&mut *tx)
                    .await?;
                tx.commit().await?;
                Ok(())
            },
        )
        .await?;
        self.find_by_id(id)
            .await?
            .ok_or_else(|| Error::not_found("user not found"))
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
